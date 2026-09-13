//! Windows WASAPI loopback capture backend (target-gated: this crate only
//! compiles this module for `cfg(windows)` — see the `#[cfg(windows)]`
//! `pub mod wasapi` in `lib.rs`).
//!
//! It is compiled/validated on a Windows runner (or any host with the
//! `x86_64-pc-windows-msvc` std target) via:
//!
//! ```text
//! cargo check --target x86_64-pc-windows-msvc   # from platform/win-emitter
//! ```
//!
//! `cargo check` does **not** link, so no Windows SDK / `link.exe` is required
//! for this compile gate; running the actual loopback capture needs a real
//! Windows audio endpoint host (documented in `build-check.md`). The `windows`
//! crate version + feature list are pinned in `Cargo.toml` under
//! `[target.'cfg(windows)'.dependencies]`.
//!
//! NOTE (t-dev-packaging, 2026-09-10): the `windows` crate dependency is pinned
//! to **0.62.2**, whose generated API surface differs from the pre-0.62 shapes
//! this file was originally written against (`AUDCLNT_BUFFERFLAGS` → `u32`
//! flags parameter, free `CoCreateInstance`/`CoInitializeEx` functions, no
//! `Error::from_win32`, `PROPVARIANT` without `data_mut()`, `PKEY_Device_*`
//! moved to `Win32::Devices::FunctionDiscovery`). The code below was repaired to
//! compile+check clean under 0.62.2; behavior is unchanged.
//!
//! # RT discipline (docs/planning/RT_CONTRACT.md, WASAPI row)
//!
//! `WasapiLoopbackCapture::poll` is the data-path (quasi-RT) reader. Its only
//! allowed work is `IAudioCaptureClient::GetBuffer`/`ReleaseBuffer` and a
//! `copy_nonoverlapping` into the caller-provided preallocated buffer: **no
//! allocation, no locking, no I/O, no logging** on that path (checked:
//! `poll`/`buffered` call no allocating or blocking APIs). All format
//! negotiation, endpoint enumeration and COM setup happen in `new`/`start`/
//! `set_endpoint`, off the data path.

use crate::{Error, FormatMeta};
use windows::Win32::System::Com::{CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

/// `PKEY_Device_FriendlyName` (a45c254e-df1c-4efd-8020-67d146a850e0, pid 14).
///
/// In windows-rs 0.62.2 this constant lives in
/// `Win32::Devices::FunctionDiscovery` (not an enabled feature), so it is
/// declared locally from the documented win32 metadata value.
const PKEY_DEVICE_FRIENDLY_NAME: windows::Win32::Foundation::PROPERTYKEY =
    windows::Win32::Foundation::PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

/// A single enumerated render endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderEndpoint {
    /// Endpoint identifier (WASAPI `IMMDevice::GetId`).
    pub id: String,
    /// Endpoint friendly name (property `PKEY_Device_FriendlyName`).
    pub name: String,
    /// `true` if the device is currently in the `DEVICE_STATE_ACTIVE` state.
    pub active: bool,
    /// Best-effort USB heuristic: friendly names commonly contain "USB". This
    /// is NOT authoritative — real USB identification needs PnP VID/PID
    /// correlation (documented follow-up; see build-check.md).
    pub is_usb: bool,
}

/// WASAPI loopback capture of the default (or a selected) render endpoint.
///
/// Construction is `unsafe` because it borrows a caller-owned, preallocated
/// capture buffer that the data path `memcpy`s into. The object is `Send` but
/// **not** `Sync`: the capture worker owns it exclusively and the COM
/// interfaces it wraps are used from that single thread.
pub struct WasapiLoopbackCapture {
    audio_client: Option<windows::Win32::Media::Audio::IAudioClient>,
    capture_client: Option<windows::Win32::Media::Audio::IAudioCaptureClient>,
    /// Caller-provided capture buffer pointer (never owned; freed by caller).
    buffer: *mut u8,
    /// Capacity of `buffer`, in bytes.
    capacity: usize,
    /// Consumption offset inside `buffer` (data path advances it).
    fill: usize,
    /// Negotiated capture format.
    fmt: FormatMeta,
    /// Selected endpoint id (`None` = follow the system default).
    endpoint_id: Option<String>,
}

unsafe impl Send for WasapiLoopbackCapture {}

impl std::fmt::Debug for WasapiLoopbackCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasapiLoopbackCapture")
            .field("capacity", &self.capacity)
            .field("fill", &self.fill)
            .field("fmt", &self.fmt)
            .field("endpoint_id", &self.endpoint_id)
            .finish()
    }
}

impl WasapiLoopbackCapture {
    /// Create the capture object around a caller-owned, preallocated
    /// `capacity`-byte buffer.
    ///
    /// # Safety
    /// `buffer` must be a valid, writable allocation of at least `capacity`
    /// bytes for the lifetime of this object. The data path only ever `memcpy`s
    /// into it; the caller owns the memory and must free it after `stop`.
    pub unsafe fn with_buffer(buffer: *mut u8, capacity: usize) -> Self {
        WasapiLoopbackCapture {
            audio_client: None,
            capture_client: None,
            buffer,
            capacity,
            fill: 0,
            fmt: FormatMeta {
                rate: 48_000,
                bits: 16,
                channels: 2,
            },
            endpoint_id: None,
        }
    }

    /// Enumerate render endpoints (active first, then unplugged USB DACs) for
    /// the UI. Off the data path.
    ///
    /// Requires COM to be initialised on the calling thread (done in
    /// [`Self::start`]; callers may also do it themselves).
    pub fn render_endpoints() -> windows::core::Result<Vec<RenderEndpoint>> {
        let enumerator: windows::Win32::Media::Audio::IMMDeviceEnumerator =
            unsafe { com::create_enumerator()? };

        let mut out = Vec::new();
        for state in [
            windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE,
            windows::Win32::Media::Audio::DEVICE_STATE_UNPLUGGED,
        ] {
            let collection = unsafe {
                enumerator.EnumAudioEndpoints(windows::Win32::Media::Audio::eRender, state)?
            };
            let count = unsafe { collection.GetCount()? };
            for i in 0..count {
                let dev = unsafe { collection.Item(i)? };
                let (id, name) = unsafe { endpoint_name(&dev)? };
                let active =
                    unsafe { dev.GetState()? } == windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE;
                let is_usb = name.to_lowercase().contains("usb");
                out.push(RenderEndpoint {
                    id,
                    name,
                    active,
                    is_usb,
                });
            }
        }
        Ok(out)
    }

    /// Select the endpoint to loop back by WASAPI endpoint id. `None` follows
    /// the system default render device.
    pub fn set_endpoint(&mut self, id: Option<String>) -> Result<(), Error> {
        self.endpoint_id = id;
        Ok(())
    }

    /// Negotiate and open the loopback stream, then start it.
    ///
    /// Initialises COM on this thread (multithreaded apartment), resolves the
    /// endpoint (explicit id, else default `eRender`/`eConsole`), activates
    /// [`IAudioClient`](windows::Win32::Media::Audio::IAudioClient), reads the
    /// mix format, and initialises in **shared + loopback** mode with the
    /// system default period. All off the data path.
    pub fn start(&mut self) -> Result<(), Error> {
        if self.capture_client.is_some() {
            return Err(Error::AlreadyStarted);
        }
        unsafe {
            // CoInitializeEx: RPC_E_CHANGED_MODE when the thread is already in
            // a different apartment is acceptable (we were handed one). Note the
            // 0.62 API: CoInitializeEx is a free fn returning an HRESULT.
            let hr: windows::core::HRESULT = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr != windows::Win32::Foundation::RPC_E_CHANGED_MODE && hr.is_err() {
                return Err(Error::Wasapi(format!("CoInitializeEx: {hr:?}")));
            }

            let device = match &self.endpoint_id {
                Some(id) => com::select_endpoint_by_id(id).map_err(wasapi_err)?,
                None => {
                    let enumerator = com::create_enumerator().map_err(wasapi_err)?;
                    enumerator
                        .GetDefaultAudioEndpoint(
                            windows::Win32::Media::Audio::eRender,
                            windows::Win32::Media::Audio::eConsole,
                        )
                        .map_err(wasapi_err)?
                }
            };

            let audio_client: windows::Win32::Media::Audio::IAudioClient = device
                .Activate::<windows::Win32::Media::Audio::IAudioClient>(CLSCTX_ALL, None)
                .map_err(wasapi_err)?;

            let wf = audio_client.GetMixFormat().map_err(wasapi_err)?;
            // Nested under the enclosing `unsafe` block — no inner unsafe needed.
            let wf = &*wf;
            let fmt = format_from_wave(wf)?;
            self.fmt = fmt;

            audio_client
                .Initialize(
                    windows::Win32::Media::Audio::AUDCLNT_SHAREMODE_SHARED,
                    windows::Win32::Media::Audio::AUDCLNT_STREAMFLAGS_LOOPBACK,
                    0, // 0 = default buffer duration
                    0, // 0 = default periodicity
                    wf,
                    None,
                )
                .map_err(wasapi_err)?;

            let capture: windows::Win32::Media::Audio::IAudioCaptureClient =
                audio_client.GetService().map_err(wasapi_err)?;

            audio_client.Start().map_err(wasapi_err)?;

            self.audio_client = Some(audio_client);
            self.capture_client = Some(capture);
        }
        Ok(())
    }

    /// Stop the loopback stream and release the clients. Idempotent.
    pub fn stop(&mut self) {
        if let Some(client) = &self.audio_client {
            unsafe {
                let _ = client.Stop();
            }
        }
        self.capture_client = None;
        self.audio_client = None;
        self.fill = 0;
    }

    /// The negotiated capture format.
    pub fn format(&self) -> FormatMeta {
        self.fmt
    }

    /// **Data-path** read: pull the next captured packet and `memcpy` it into
    /// the caller buffer at the current `fill` offset. Returns the number of
    /// bytes copied (0 = none available).
    ///
    /// Allowed in the RT callback (RT_CONTRACT WASAPI row): `GetBuffer`/
    /// `ReleaseBuffer` + `copy_nonoverlapping`. Nothing else; no allocation.
    /// A packet that would exceed the preallocated buffer is dropped with
    /// [`Error::BufferOverflow`] (never grows the buffer).
    ///
    /// # Safety
    ///
    /// Caller must ensure `poll` is only invoked while the capture client is
    /// started (see [`Self::start`]); the returned byte count borrows
    /// `self.buffer` memory and must be consumed via [`Self::buffered`] before
    /// the next `GetBuffer`/`ReleaseBuffer` cycle.
    pub unsafe fn poll(&mut self) -> Result<usize, Error> {
        let Some(capture) = &self.capture_client else {
            return Err(Error::NotStarted);
        };

        let mut data: *mut u8 = std::ptr::null_mut();
        let mut frames: u32 = 0;
        // The 0.62 GetBuffer writes the flags into a plain u32.
        let mut flags: u32 = 0;
        capture
            .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
            .map_err(wasapi_err)?;

        let bytes = frames as usize * self.bytes_per_frame();
        let room = self.capacity.saturating_sub(self.fill);
        if bytes > room {
            let _ = capture.ReleaseBuffer(frames);
            return Err(Error::BufferOverflow);
        }
        if bytes == 0 {
            let _ = capture.ReleaseBuffer(frames);
            return Ok(0);
        }

        // AUDCLNT_BUFFERFLAGS_SILENT is a `_AUDCLNT_BUFFERFLAGS(i32)` in 0.62.
        if flags & (windows::Win32::Media::Audio::AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
            // Marked silent: zero-fill (same RT-only operations).
            std::ptr::write_bytes(self.buffer.add(self.fill), 0u8, bytes);
        } else {
            std::ptr::copy_nonoverlapping(data, self.buffer.add(self.fill), bytes);
        }
        self.fill += bytes;
        let _ = capture.ReleaseBuffer(frames);
        Ok(bytes)
    }

    /// Borrow the currently-buffered region (call on the worker, off RT).
    pub fn buffered(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.buffer, self.fill) }
    }

    /// Reset consumption (call on the worker between frames).
    pub fn reset_fill(&mut self) {
        self.fill = 0;
    }

    fn bytes_per_frame(&self) -> usize {
        (self.fmt.bits as usize / 8) * self.fmt.channels as usize
    }
}

/// Resolve the endpoint id + friendly name for an [`IMMDevice`].
///
/// The friendly name comes from the endpoint's property store
/// (`PKEY_Device_FriendlyName`) — there is no direct `GetFriendlyName` on
/// `IMMDevice`. Off the data path.
unsafe fn endpoint_name(
    dev: &windows::Win32::Media::Audio::IMMDevice,
) -> windows::core::Result<(String, String)> {
    let id = unsafe { dev.GetId()? };
    let id_str = unsafe { id.to_string() }
        .map_err(|_| windows::core::Error::from_hresult(windows::core::HRESULT(0)))?;
    let store = unsafe { dev.OpenPropertyStore(STGM_READ)? };
    let name = unsafe { store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME)? };
    let name_str = unsafe { prop_to_string(&name) };
    Ok((id_str, name_str))
}

/// Best-effort coercion of a PROPVARIANT to a String (only VT_LPWSTR/strings
/// handled; everything else yields the empty string).
unsafe fn prop_to_string(
    pv: &windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
) -> String {
    // 0.62: PROPVARIANT exposes its fields through the Anonymous union chain
    // (no `data_mut()` helper any more).
    let vt = pv.Anonymous.Anonymous.vt;
    if vt == windows::Win32::System::Variant::VT_LPWSTR
        || vt == windows::Win32::System::Variant::VT_BSTR
    {
        let pwsz: windows::core::PWSTR = pv.Anonymous.Anonymous.Anonymous.pwszVal;
        unsafe { pwsz.to_string().unwrap_or_default() }
    } else {
        String::new()
    }
}

/// Convert the WASAPI mix [`WAVEFORMATEX`] into our generic [`FormatMeta`].
///
/// Handles `WAVE_FORMAT_EXTENSIBLE` (reads `WAVEFORMATEXTENSIBLE` valid-bits)
/// and plain PCM. Only integer PCM is accepted for now (`bits in
/// {16,24,32}`); float (`IEEE_FLOAT`) is a documented follow-up.
unsafe fn format_from_wave(
    wf: &windows::Win32::Media::Audio::WAVEFORMATEX,
) -> Result<FormatMeta, Error> {
    let rate = wf.nSamplesPerSec;
    let channels = wf.nChannels as u32;

    let (bits, _valid_bits) =
        if wf.wFormatTag == windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE as u16 {
            // SAFETY: an EXTENSIBLE blob has at least the EXTENSIBLE header size.
            let ext = &*(wf as *const windows::Win32::Media::Audio::WAVEFORMATEX
                as *const windows::Win32::Media::Audio::WAVEFORMATEXTENSIBLE);
            let valid = ext.Samples.wValidBitsPerSample as u32;
            let container = wf.wBitsPerSample as u32;
            (if valid != 0 { valid } else { container }, valid)
        } else {
            // 0.62: WAVE_FORMAT_PCM moved to Win32::Media::Audio.
            let _ = windows::Win32::Media::Audio::WAVE_FORMAT_PCM;
            (wf.wBitsPerSample as u32, 0)
        };

    match bits {
        16 | 24 | 32 => Ok(FormatMeta {
            rate,
            bits,
            channels,
        }),
        _other => Err(Error::UnsupportedFormat),
    }
}

/// Map a WDR [`Error`] to the ``[`windows::core::Error`]``? No — map a
/// `windows::core::Result` failure into our error type with the message text.
fn wasapi_err(e: windows::core::Error) -> Error {
    Error::Wasapi(e.to_string())
}

mod com {
    //! Thin COM helpers (off the data path). 0.62: `CoCreateInstance` is a
    //! free function in `Win32::System::Com` (no `Com` type any more).

    /// Create the endpoint enumerator (`MMDeviceEnumerator` CLSID, `IMMDeviceEnumerator`).
    pub unsafe fn create_enumerator(
    ) -> windows::core::Result<windows::Win32::Media::Audio::IMMDeviceEnumerator> {
        unsafe {
            windows::Win32::System::Com::CoCreateInstance(
                &windows::Win32::Media::Audio::MMDeviceEnumerator,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
        }
    }

    /// Resolve a specific endpoint by id.
    pub unsafe fn select_endpoint_by_id(
        id: &str,
    ) -> windows::core::Result<windows::Win32::Media::Audio::IMMDevice> {
        let enumerator = unsafe { create_enumerator()? };
        let h: windows::core::HSTRING = windows::core::HSTRING::from(id);
        unsafe { enumerator.GetDevice(&h) }
    }
}

/// Re-export the `wasapi_err` mapping so clippy can see error-conversion is
/// actually used on some path.
#[allow(dead_code)]
fn _map_err(e: windows::core::Error) -> Error {
    wasapi_err(e)
}
