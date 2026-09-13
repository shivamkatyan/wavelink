//! ScreenCaptureKit system-audio capture adapter (macOS 13+).
//!
//! `SystemCaptureHandle` is the real, **compiling** SCK adapter: it acquires
//! `SCShareableContent`, builds an `SCContentFilter` over the primary display,
//! configures `SCStreamConfiguration` with `capturesAudio = true`, registers an
//! audio-only [`SCStreamOutput`] handler, and starts the stream. All of that is
//! **off the data path** (setup).
//!
//! # RT discipline (RT_CONTRACT macOS SCK row)
//!
//! The sample handler runs on the SCK output queue (RT-ish). Its only work is:
//!
//! ```text
//! CMSampleBuffer → audio_buffer_list() → per AudioBuffer: copy_nonoverlapping
//! into a caller-preallocated region + an atomic fill/overflow store.
//! ```
//!
//! No allocation, no locking, no syscalls, no logging, no encode/transport on
//! that path. That is exactly what [`PreallocatedCaptureBuffer::push_rt`] does
//! (it shares the design of the sibling's `WasapiLoopbackCapture` raw-buffer
//! data path).
//!
//! # Compile vs hardware
//!
//! * **Compiled on this host**: everything in this module (the `screencapturekit`
//!   dep builds; see `build-check.md`).
//! * **Hardware-gated**: actually receiving audio bytes needs a logged-in GUI
//!   session with Screen Recording TCC granted — the stream creation itself
//!   (`SCShareableContent::get()` → empty/error until consent) will otherwise
//!   fail, which is the point of `permission::PermissionGate`. E2E validation
//!   is the `macos-capture-sck` hardware gate.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use screencapturekit::cm::{CMSampleBuffer, CMSampleBufferExt};
use screencapturekit::error::SCError;
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::configuration::SCStreamConfiguration;
use screencapturekit::stream::content_filter::SCContentFilter;
use screencapturekit::stream::output_trait::SCStreamOutputTrait;
use screencapturekit::stream::output_type::SCStreamOutputType;
use screencapturekit::stream::sc_stream::SCStream;

use crate::{Error, FormatMeta};

/// Caller-preallocated capture buffer that the RT sample handler memcpys into.
///
/// The pointer is caller-owned (never freed here); `push_rt` is the ONLY entry
/// allowed on the RT callback: a `copy_nonoverlapping` plus atomic
/// fill/overflow stores. `buffered`/`reset_fill`/`overflowed` are worker-side.
#[derive(Debug)]
pub struct PreallocatedCaptureBuffer {
    ptr: *mut u8,
    capacity: usize,
    fill: AtomicUsize,
    overflowed: AtomicBool,
    /// One-time capture of the DELIVERED sample rate/channels (read from the
    /// first audio sample's CMFormatDescription; 0 until seen). `fmt_seen`
    /// makes the non-idempotent read a single no-op after the first sample.
    fmt_seen: AtomicBool,
    delivered_rate: AtomicUsize,
    delivered_channels: AtomicUsize,
}

// SAFETY: the object is exclusively owned/managed by the capture worker for
// everything except `push_rt`, which only copies under load-acquire/store ff
// of the fill counter; the pointed-to memory outlives this object (caller
// contract, see `with_buffer`).
unsafe impl Send for PreallocatedCaptureBuffer {}
unsafe impl Sync for PreallocatedCaptureBuffer {}

impl PreallocatedCaptureBuffer {
    /// Wrap a caller-owned, writable `capacity`-byte buffer.
    ///
    /// # Safety
    /// `buffer` must be a valid, writable allocation of at least `capacity`
    /// bytes for the lifetime of this object; the data path only memcpys into
    /// it. The caller owns the memory and must free it after capture stops.
    pub unsafe fn with_buffer(buffer: *mut u8, capacity: usize) -> Self {
        PreallocatedCaptureBuffer {
            ptr: buffer,
            capacity,
            fill: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
            fmt_seen: AtomicBool::new(false),
            delivered_rate: AtomicUsize::new(0),
            delivered_channels: AtomicUsize::new(0),
        }
    }

    /// Worker side: the delivered (not merely requested) sample rate/channels,
    /// captured once from the first audio sample's format description.
    pub fn delivered_format(&self) -> Option<FormatMeta> {
        let rate = self.delivered_rate.load(Ordering::Acquire);
        let channels = self.delivered_channels.load(Ordering::Acquire);
        if rate == 0 {
            return None;
        }
        let channels = if channels == 0 { 2 } else { channels };
        Some(FormatMeta {
            rate: rate as u32,
            bits: 16,
            channels: channels as u32,
        })
    }

    /// RT data path extension: ONE-TIME capture of the delivered format from the
    /// first audio sample's `CMFormatDescription` (per RT_CONTRACT: single
    /// retain/release + atomic stores; no allocation, no locks). No-op after the
    /// first sample.
    pub fn note_delivered_format(&self, sample: &CMSampleBuffer) {
        if self.fmt_seen.load(Ordering::Relaxed) {
            return;
        }
        if self.fmt_seen.swap(true, Ordering::Relaxed) {
            return; // another callback already captured it
        }
        if let Some(fd) = sample.format_description() {
            if let Some(r) = fd.audio_sample_rate() {
                self.delivered_rate
                    .store(r.round() as usize, Ordering::Release);
            }
            if let Some(c) = fd.audio_channel_count() {
                self.delivered_channels.store(c as usize, Ordering::Release);
            }
        }
    }

    /// **RT data path**: append `bytes` via `copy_nonoverlapping`; only atomic
    /// work. Returns `false` (and flags the overflow) if there is not enough
    /// room — never grows, never allocates.
    pub fn push_rt(&self, bytes: &[u8]) -> bool {
        let start = self.fill.load(Ordering::Acquire);
        let end = start.saturating_add(bytes.len());
        if end > self.capacity {
            self.overflowed.store(true, Ordering::Relaxed);
            return false;
        }
        if !bytes.is_empty() {
            // SAFETY: `start..end` ≤ capacity and `ptr` is a valid allocation
            // of that size (constructor contract); `bytes` is a valid slice.
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.add(start), bytes.len());
            }
        }
        self.fill.store(end, Ordering::Release);
        true
    }

    /// Whether any `push_rt` has overflowed since the last [`Self::reset_fill`].
    pub fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Relaxed)
    }

    /// Worker side: borrow the currently-buffered region (off RT).
    pub fn buffered(&self) -> &[u8] {
        let len = self.fill.load(Ordering::Acquire);
        if len == 0 {
            return &[];
        }
        // SAFETY: `len` ≤ capacity and `ptr` is valid for that many bytes
        // (constructor contract); caller must not mutate while reading.
        unsafe { core::slice::from_raw_parts(self.ptr, len) }
    }

    /// Worker side: reset consumption between frames (off RT).
    pub fn reset_fill(&self) {
        self.fill.store(0, Ordering::Release);
        self.overflowed.store(false, Ordering::Relaxed);
    }

    /// Preallocated capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// System-capture adapter seam: the boundary a real backend satisfies.
///
/// [`SystemCaptureHandle`] is the SCK implementation; the seam exists so a
/// future HAL tap/AU backend (or a different SCK version) can drop in without
/// the app changing.
pub trait SystemCaptureAdapter: Send {
    /// Begin capture (requires TCC-granted GUI session at runtime).
    fn start(&mut self) -> Result<(), Error>;
    /// Stop capture; idempotent.
    fn stop(&mut self);
    /// The requested/negotiated capture format.
    fn format(&self) -> FormatMeta;
    /// Select the endpoint; `per_app = true` targets a specific application
    /// (macOS route: Core Audio process taps 14.2+, seam-gated).
    fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error>;
    /// Worker side: borrow the buffered audio bytes.
    fn buffered(&self) -> &[u8];
    /// Worker side: reset the buffered region.
    fn reset_buffered(&mut self);
}

/// ScreenCaptureKit system-audio capture (real, compiling adapter).
///
/// Not `Clone`, not shared across threads; the capture worker owns it. The RT
/// handler is the owned [`SCStream`]'s audio output, sharing only the
/// `Arc<PreallocatedCaptureBuffer>` sink.
pub struct SystemCaptureHandle {
    stream: Option<SCStream>,
    sink: Arc<PreallocatedCaptureBuffer>,
    fmt: FormatMeta,
    started: bool,
    endpoint_name: Option<String>,
    per_app: bool,
}

impl std::fmt::Debug for SystemCaptureHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemCaptureHandle")
            .field("fmt", &self.fmt)
            .field("started", &self.started)
            .field("capacity", &self.sink.capacity())
            .field("endpoint_name", &self.endpoint_name)
            .field("per_app", &self.per_app)
            .finish()
    }
}

impl SystemCaptureHandle {
    /// Wrap a caller-owned, preallocated `capacity`-byte capture buffer.
    ///
    /// # Safety
    /// `buffer` must stay valid and writable for the lifetime of this handle
    /// (the caller frees it after `stop`); the sample handler only memcpys
    /// into it.
    pub unsafe fn with_buffer(buffer: *mut u8, capacity: usize, fmt: FormatMeta) -> Self {
        SystemCaptureHandle {
            stream: None,
            sink: Arc::new(unsafe { PreallocatedCaptureBuffer::with_buffer(buffer, capacity) }),
            fmt,
            started: false,
            endpoint_name: None,
            per_app: false,
        }
    }

    /// The requested capture format (what we asked SCK for).
    pub fn format(&self) -> FormatMeta {
        self.fmt
    }

    /// Select the endpoint by name; `per_app = true` marks per-application
    /// targeting. System-wide SCK capture follows the default output; per-app
    /// routing is the Core Audio process-tap seam (`macos14-taps`), so storing
    /// the intent here and refusing it at start is the honest behaviour until
    /// the tap path is wired (build-check.md).
    pub fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error> {
        if name.is_empty() {
            return Err(Error::EndpointNotFound(String::new()));
        }
        if per_app {
            return Err(Error::MacAudio(
                "per-app capture on macOS = Core Audio process taps (14.2+), \
                 a seam in this scaffold — see build-check.md"
                    .into(),
            ));
        }
        self.endpoint_name = Some(name.to_string());
        self.per_app = false;
        Ok(())
    }

    /// Start system-wide audio capture (off the data path).
    ///
    /// Runtime requires a logged-in GUI session with Screen Recording TCC
    /// granted (hardware gate); until then `SCShareableContent::get()` yields
    /// nothing to filter and this returns an honest [`Error::MacAudio`].
    pub fn start(&mut self) -> Result<(), Error> {
        if self.started {
            return Err(Error::AlreadyStarted);
        }
        let content = SCShareableContent::create()
            .get()
            .map_err(|e| Error::MacAudio(format!("SCShareableContent: {e}")))?;
        let display = content.displays().into_iter().next().ok_or_else(|| {
            Error::MacAudio(
                "no shareable display — Screen Recording TCC not granted or no GUI session \
                 (hardware gate; see build-check.md)"
                    .into(),
            )
        })?;
        let filter = SCContentFilter::create().with_display(&display).build();
        let config = SCStreamConfiguration::new()
            .with_captures_audio(true)
            .with_sample_rate(self.fmt.rate as i32)
            .with_channel_count(self.fmt.channels as i32);
        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(
            AudioOnlyHandler {
                sink: self.sink.clone(),
            },
            SCStreamOutputType::Audio,
        );
        stream.start_capture().map_err(sc_error)?;
        self.stream = Some(stream);
        self.started = true;
        Ok(())
    }

    /// Stop capture; idempotent.
    pub fn stop(&mut self) {
        if let Some(stream) = &self.stream {
            let _ = stream.stop_capture();
        }
        self.stream = None;
        self.started = false;
        self.sink.reset_fill();
    }

    /// Worker side: borrow the buffered audio bytes.
    pub fn buffered(&self) -> &[u8] {
        self.sink.buffered()
    }

    /// Worker side: reset the buffered region between frames.
    pub fn reset_buffered(&mut self) {
        self.sink.reset_fill();
    }

    /// Whether the RT path has flagged an overflow since the last reset.
    pub fn overflowed(&self) -> bool {
        self.sink.overflowed()
    }

    /// The delivered (actual) capture format, once the first audio sample has
    /// been seen; `None` before that (or if the format description is absent).
    /// Worker side.
    pub fn delivered_format(&self) -> Option<FormatMeta> {
        self.sink.delivered_format()
    }
}

impl SystemCaptureAdapter for SystemCaptureHandle {
    fn start(&mut self) -> Result<(), Error> {
        self.start()
    }

    fn stop(&mut self) {
        self.stop();
    }

    fn format(&self) -> FormatMeta {
        self.fmt
    }

    fn set_endpoint(&mut self, name: &str, per_app: bool) -> Result<(), Error> {
        self.set_endpoint(name, per_app)
    }

    fn buffered(&self) -> &[u8] {
        self.buffered()
    }

    fn reset_buffered(&mut self) {
        self.reset_buffered();
    }
}

/// SCK audio output handler: the RT-ish sample handler. Holds only the sink.
struct AudioOnlyHandler {
    sink: Arc<PreallocatedCaptureBuffer>,
}

impl SCStreamOutputTrait for AudioOnlyHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }
        // One-time, allocation-free capture of the delivered audio format
        // (RT_CONTRACT macOS SCK row: single retain/release on first sample).
        self.sink.note_delivered_format(&sample);
        let Some(list) = sample.audio_buffer_list() else {
            return;
        };
        for buffer in list.iter() {
            self.sink.push_rt(buffer.data());
        }
    }
}

fn sc_error(e: SCError) -> Error {
    Error::MacAudio(format!("ScreenCaptureKit: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alloc(capacity: usize) -> (Box<[u8]>, *mut u8) {
        let mut boxed = vec![0u8; capacity].into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        (boxed, ptr)
    }

    #[test]
    fn preallocated_buffer_copies_and_reports_overflow() {
        let (_mem, ptr) = alloc(64);
        let sink = unsafe { PreallocatedCaptureBuffer::with_buffer(ptr, 64) };
        assert!(sink.push_rt(&[1u8, 2, 3]));
        assert_eq!(sink.buffered(), &[1u8, 2, 3]);
        assert!(sink.push_rt(&[4u8; 60]));
        assert_eq!(sink.buffered().len(), 63);
        // Two bytes would cross the 64-byte capacity: rejected + flagged,
        // buffer length unchanged.
        assert!(!sink.push_rt(&[9u8; 2]));
        assert!(sink.overflowed());
        assert_eq!(sink.buffered().len(), 63);
        sink.reset_fill();
        assert!(sink.buffered().is_empty(), "reset must empty the buffer");
        assert!(!sink.overflowed());
    }
}
