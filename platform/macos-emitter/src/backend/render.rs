//! macOS Core Audio output render sink (WS3 — desktop receiver render path).
//!
//! The macOS shell receives through the proven
//! [`wdr_refsim::sink::QuicRenderReceiver`] seam and renders into a
//! [`wdr_refsim::sink::RenderSink`]. This module is that sink's macOS Audio
//! face: it **preflights a real HAL output device** (the same `hal.rs`
//! property reads that are live-validated for enumeration) and then fails with
//! a typed, honest error when the actual playback I/O would not be
//! verifiable.
//!
//! Honesty boundary (evidence-before-claims): the seam + live receiver + the
//! host-testable null sink are proven by
//! `scripts/verify/macos-receive-smoke.sh` (canonical golden hash, no
//! hardware). *Playing* bytes out of a HAL output AudioUnit is **not** claimed
//! as verified here: it needs a physical output session on this machine (or a
//! TCC/USB-DAC session), which is the documented **external gate** named in
//! RELEASE_STATUS "macOS desktop Receiver render path". Until that gate
//! passes, `--sink audio` fails loudly at preflight rather than pretending to
//! play.
//!
//! ## What the gated AudioUnit playback path will implement (runbook)
//!
//! 1. Open a default-output `AudioUnit` (`kAudioUnitSubType_HALOutput`), set
//!    the current device from [`hal::default_output_device_id`].
//! 2. Configure the client `AudioStreamBasicDescription` from `SinkFormat`
//!    (rate/channels/i16 or 24-bit) on `kAudioUnitScope_Input, bus 0`.
//! 3. Install a render callback (`kAudioUnitProperty_SetRenderCallback`) whose
//!    RT body memcpys from a preallocated SPSC ring filled by `on_block` on the
//!    worker (the RT_CONTRACT discipline — no alloc/lock/syscall on the audio
//!    callback).
//! 4. `AudioUnitInitialize` + `AudioOutputUnitStart`; `finish()` flushes and
//!    stops.
//!
//! This is deliberately **not** written blind: step 3's RT callback is exactly
//! the kind of code this repo validates on hardware (per RT_CONTRACT.md
//! §2 table + HARDWARE_VALIDATION.md), never shipped un-run.

use wdr_proto::SampleRepr;
use wdr_refsim::sink::{RenderSink, SinkError, SinkFormat};

use crate::backend::hal;

/// The macOS output render sink. Constructing it preflights the default HAL
/// output device; rendering audio out of it is the device-gated half of WS3
/// (typed error until the gate passes).
#[derive(Debug)]
pub struct CoreAudioRenderSink {
    /// The preflighted default HAL output device id (non-zero).
    device_id: u32,
    device_name: String,
    /// The negotiated decoded output format (set once by `on_format`).
    format: Option<SinkFormat>,
}

impl CoreAudioRenderSink {
    /// Preflight: require a default HAL output device that is output-capable.
    /// Errors loudly when there is none (headless host, or no audio session) —
    /// the run never pretends to play.
    pub fn try_new() -> Result<Self, SinkError> {
        let device_id = hal::default_output_device_id()
            .map_err(|e| SinkError::Format(format!("HAL preflight: {e}")))?
            .ok_or_else(|| {
                SinkError::Format(
                    "no default HAL output device — desktop receiver audio out requires an \
                     output device / USB DAC (device gate `usb-dac-device`); use --sink null \
                     for the host-verified lossless hash path"
                        .into(),
                )
            })?;
        if !hal::output_capable(device_id)
            .map_err(|e| SinkError::Format(format!("HAL preflight: {e}")))?
        {
            return Err(SinkError::Format(format!(
                "default HAL output device {device_id} is not output-capable"
            )));
        }
        let device_name =
            hal::device_name(device_id).unwrap_or_else(|_| format!("device {device_id}"));
        Ok(Self {
            device_id,
            device_name,
            format: None,
        })
    }
}

impl RenderSink for CoreAudioRenderSink {
    fn on_format(&mut self, fmt: SinkFormat) -> Result<(), SinkError> {
        self.format = Some(fmt);
        Ok(())
    }

    fn on_block(&mut self, _bytes: &[u8]) -> Result<(), SinkError> {
        // The AudioUnit playback path (runbook above) is the `usb-dac-device` /
        // TCC-session external gate. Until it passes on real hardware this sink
        // reports the gate loudly instead of faking playback.
        let fmt = self.format.unwrap_or(SinkFormat::canonical());
        Err(SinkError::Format(format!(
            "Core Audio playback I/O is device-gated (WS3): decoded {}-bit/{}Hz/{}ch output to HAL \
             device {}/{} is not yet verified on hardware — use --sink null for the verified \
             lossless path",
            depth_name(fmt.sample_repr),
            fmt.sample_rate,
            fmt.channels,
            self.device_id,
            self.device_name,
        )))
    }

    fn finish(&mut self) -> Result<(), SinkError> {
        Ok(())
    }
}

fn depth_name(r: SampleRepr) -> &'static str {
    match r {
        SampleRepr::I16 => "16",
        SampleRepr::I24Packed => "24",
        SampleRepr::F32 => "f32",
        SampleRepr::I32 => "32",
    }
}
