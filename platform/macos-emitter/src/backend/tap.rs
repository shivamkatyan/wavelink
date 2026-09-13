//! Core Audio process taps — per-application capture on macOS 14.2
//! (feature `macos14-taps`, ADR-008 / PLATFORM_MATRIX §A).
//!
//! # What is real here
//!
//! The two C functions below are pinned by the macOS 26 SDK header
//! `CoreAudio.framework/Headers/AudioHardwareTapping.h`:
//!
//! ```c
//! OSStatus AudioHardwareCreateProcessTap(CATapDescription* inDescription,
//!                                        AudioObjectID*   outTapID)
//!                                       API_AVAILABLE(macos(14.2))
//!                                       API_UNAVAILABLE(ios, watchos, tvos);
//! OSStatus AudioHardwareDestroyProcessTap(AudioObjectID inTapID)
//!                                       API_AVAILABLE(macos(14.2));
//! ```
//!
//! These declarations **link cleanly on this host** (the `_AudioHardwareCreateProcessTap` /
//! `_AudioHardwareDestroyProcessTap` symbols resolve against CoreAudio on
//! macOS 26) — that linkage is asserted by the `tap_ffi_symbols_link` test,
//! giving a real, honest compile+link gate for the FFI we do ship.
//!
//! # What is a seam here (honest)
//!
//! The first parameter is an **Objective-C `CATapDescription*`** (an `NSObject`
//! subclass constructed via ObjC message sends: `initStereoMixdownOfProcesses:`
//! / `initWithProcesses:andDeviceUID:withStream:` and the `privateTap`/
//! `muteBehavior` properties). There is no C-struct form, so a pure-Rust caller
//! cannot construct a working tap description without an ObjC-runtime shim
//! (`objc2` messaging + `NSArray`/`NSNumber`/`NSString` bridges). Building that
//! shim correctly and validating it on a real 14.2+ box is the O-S
//! (open-source) follow-up; until then:
//!
//! * [`ProcessTapHandle`] is a lifecycle seam around a tap id handed in from
//!   outside (e.g. an ObjC/Swift shim), with the same start/stop semantics as
//!   the rest of the crate and the RT preallocated-copy discipline documented
//!   for whenever the read path is wired.
//! * `AudioHardwareCreateProcessTap` is **never called** with a fabricated
//!   description (nothing to pass); it is only address-checked (link gate).
// Runbook + honest O-S gate marking: see `build-check.md`.

use coreaudio_sys::{AudioObjectID, OSStatus};

use crate::Error;

// Raw FFI pinned to `AudioHardwareTapping.h` (macOS 14.2+). `in_description`
// is an ObjC `CATapDescription*` — see module docs; never called from here
// except through the future seam shim.
#[allow(dead_code)] // link-gated; called only by the future tap shim
unsafe extern "C" {
    fn AudioHardwareCreateProcessTap(
        in_description: *const core::ffi::c_void,
        out_tap_id: *mut AudioObjectID,
    ) -> OSStatus;
    fn AudioHardwareDestroyProcessTap(in_tap_id: AudioObjectID) -> OSStatus;
}

/// Description of a per-application tap this adapter is asked to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapSpec {
    /// Application the tap targets (display name or bundle id).
    pub app_name: String,
    /// Optional pid of the app (0 = resolve by `app_name`).
    pub pid: Option<u32>,
    /// True = global tap (all processes) rather than a single-app tap.
    pub global: bool,
}

/// Process-tap adapter seam.
pub trait ProcessTap {
    /// Begin delivering this tap's audio (RT preallocated-copy path once the
    /// reading side is wired). `AlreadyStarted` on double start.
    fn start(&mut self) -> Result<(), Error>;
    /// Stop and mark idle; idempotent. Does NOT destroy the underlying tap.
    fn stop(&mut self);
}

/// Lifecycle seam around a process tap's `AudioObjectID`.
///
/// `tap_id` comes from an externally-created tap (an ObjC/Swift `CATapDescription`
/// shim); this handle owns the destroy side of the lifecycle. The read side
/// (per-UID PCM into the preallocated buffer) is the follow-up seam.
#[derive(Debug)]
pub struct ProcessTapHandle {
    tap_id: Option<AudioObjectID>,
    spec: Option<TapSpec>,
    running: bool,
}

impl ProcessTapHandle {
    /// Attach to an externally-created process tap id.
    pub fn for_tap_id(tap_id: AudioObjectID) -> Self {
        ProcessTapHandle {
            tap_id: Some(tap_id),
            spec: None,
            running: false,
        }
    }

    /// A handle with no tap attached (before creation — the seam boundary).
    pub fn detached() -> Self {
        ProcessTapHandle {
            tap_id: None,
            spec: None,
            running: false,
        }
    }

    /// Record which app this handle is meant for. Does not create anything —
    /// creation is the ObjC seam (build-check.md runbook).
    pub fn install_spec(&mut self, spec: TapSpec) {
        self.spec = Some(spec);
    }

    /// The attached tap id, if any.
    pub fn tap_id(&self) -> Option<AudioObjectID> {
        self.tap_id
    }

    /// Destroy the underlying tap (`AudioHardwareDestroyProcessTap`), tearing
    /// down the read side. Idempotent: the second call is a no-op.
    pub fn destroy(&mut self) -> Result<(), Error> {
        let Some(id) = self.tap_id.take() else {
            return Ok(());
        };
        // SAFETY: `id` came from `AudioHardwareCreateProcessTap` (an external
        // shim) and we never destroy it twice (`take` above).
        let status = unsafe { AudioHardwareDestroyProcessTap(id) };
        if status != 0 {
            self.tap_id = Some(id); // restore so a retry is possible
            return Err(Error::MacAudio(format!(
                "AudioHardwareDestroyProcessTap: OSStatus {status}"
            )));
        }
        self.running = false;
        Ok(())
    }
}

impl ProcessTap for ProcessTapHandle {
    fn start(&mut self) -> Result<(), Error> {
        if self.running {
            return Err(Error::AlreadyStarted);
        }
        if self.tap_id.is_none() {
            // Honest refusal: nothing is created yet (ObjC seam), so there is
            // nothing to read. Fail closed rather than pretend.
            return Err(Error::MacAudio(
                "no process tap created — CATapDescription construction is the ObjC seam \
                 (macOS 14.2+ hardware gate, build-check.md)"
                    .into(),
            ));
        }
        // Once an id exists and the read seam is wired, the worker would drain
        // the tap's PCM into the same PreallocatedCaptureBuffer path used by
        // system_capture. `running` records the client's intent.
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forces the two raw symbols to be resolved at link time — the honest
    /// compile+link gate for the FFI skeleton (never calls them). If the
    /// symbols were not exported by CoreAudio on the build host, linking the
    /// test binary would fail here.
    #[test]
    fn tap_ffi_symbols_link() {
        let create: unsafe extern "C" fn(*const core::ffi::c_void, *mut u32) -> i32 =
            AudioHardwareCreateProcessTap;
        let destroy: unsafe extern "C" fn(u32) -> i32 = AudioHardwareDestroyProcessTap;
        // Binding the function items alone forces the linker to resolve the
        // symbols (a function-pointer value cannot be formed otherwise).
        let _ = (create, destroy);
    }

    #[test]
    fn detached_handle_refuses_to_start() {
        let mut handle = ProcessTapHandle::detached();
        handle.install_spec(TapSpec {
            app_name: "Music".into(),
            pid: None,
            global: false,
        });
        assert!(matches!(
            handle.start(),
            Err(Error::MacAudio(_)) | Err(Error::NotStarted)
        ));
        assert!(!handle.running);
    }

    #[test]
    fn start_stop_lifecycle_records_intent() {
        let mut handle = ProcessTapHandle::for_tap_id(0x9);
        assert_eq!(handle.start(), Ok(()));
        assert_eq!(handle.start(), Err(Error::AlreadyStarted));
        handle.stop();
        assert_eq!(handle.start(), Ok(()));
        // destroy on the fake id is intentionally NOT exercised (real OS call);
        // idempotent double-destroy of an already-detached handle is safe:
        let mut detached = ProcessTapHandle::detached();
        assert_eq!(detached.destroy(), Ok(()));
        assert_eq!(detached.destroy(), Ok(()));
    }
}
