//! USB DAC / device hotplug detection (macOS HAL).
//!
//! `HALDeviceMonitor` registers `AudioObjectAddPropertyListener` on the system
//! object for `kAudioHardwarePropertyDevices`. The C callback is invoked on
//! the HAL notification thread and does **only an atomic generation bump** (no
//! allocation / locking / I/O), consistent with RT discipline. The worker calls
//! [`DeviceMonitor::poll_changes`], which re-enumerates and diffs against the
//! baseline to emit granular add/remove [`crate::RouteChange`]s.
//!
//! We use the **function-pointer** listener form
//! (`AudioObjectAddPropertyListener`, a plain C callback) rather than the block
//! form (`AudioObjectAddPropertyListenerBlock`, which needs a libdispatch block
//! literal). Both are declared by `coreaudio-sys`; the function-pointer form is
//! the one that compiles and links cleanly in pure Rust on this host. Block-form
//! delivery is a documented follow-up (build-check.md).
//!
//! Live end-to-end hotplug delivery needs a physical device to be
//! plugged/unplugged in a logged-in session — hardware gate (build-check.md).

use std::sync::atomic::{AtomicU32, Ordering};

use coreaudio_sys::*;

use super::hal;
use crate::{Error, RouteChange};

/// Bumped by the HAL listener callback on any change to the device list.
static DEVICE_GENERATION: AtomicU32 = AtomicU32::new(0);

/// HAL `kAudioHardwarePropertyDevices` change callback. Runs on the HAL
/// notification thread; only atomic work here.
unsafe extern "C" fn devices_listener(
    _in_object_id: AudioObjectID,
    _in_number_addresses: u32,
    _in_addresses: *const AudioObjectPropertyAddress,
    _in_client_data: *mut core::ffi::c_void,
) -> i32 {
    DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed);
    0
}

/// Hotplug subscription boundary (seam, real-impl-able; see `HALDeviceMonitor`).
pub trait DeviceMonitor {
    /// Subscribe to device-list changes. Idempotent. Off the data path.
    fn start_monitoring(&mut self) -> Result<(), Error>;
    /// Unsubscribe. Idempotent.
    fn stop_monitoring(&mut self);
    /// Poll for added/removed devices since the last poll (worker side).
    fn poll_changes(&mut self) -> Vec<RouteChange>;
}

/// Real HAL hotplug monitor (function-pointer listener form).
pub struct HALDeviceMonitor {
    subscribed: bool,
    last_generation: u32,
    baseline: Vec<(u32, String)>,
    registration_address: AudioObjectPropertyAddress,
}

impl HALDeviceMonitor {
    /// New monitor (unsubscribed, no baseline).
    pub fn new() -> Self {
        HALDeviceMonitor {
            subscribed: false,
            last_generation: 0,
            baseline: Vec::new(),
            registration_address: AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDevices,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            },
        }
    }

    fn refresh_baseline(&mut self) {
        self.baseline = hal::enumerate_output_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();
    }
}

impl Default for HALDeviceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceMonitor for HALDeviceMonitor {
    fn start_monitoring(&mut self) -> Result<(), Error> {
        if self.subscribed {
            return Ok(());
        }
        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject,
                &self.registration_address,
                Some(devices_listener),
                core::ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(Error::MacAudio(format!(
                "AudioObjectAddPropertyListener: OSStatus {status}"
            )));
        }
        self.subscribed = true;
        // Snapshot the current device list so the first poll is a no-change
        // baseline, not a burst of "Added" events.
        self.refresh_baseline();
        self.last_generation = DEVICE_GENERATION.load(Ordering::Relaxed);
        Ok(())
    }

    fn stop_monitoring(&mut self) {
        if !self.subscribed {
            return;
        }
        unsafe {
            let _ = AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject,
                &self.registration_address,
                Some(devices_listener),
                core::ptr::null_mut(),
            );
        }
        self.subscribed = false;
    }

    fn poll_changes(&mut self) -> Vec<RouteChange> {
        if !self.subscribed {
            return Vec::new();
        }
        let generation = DEVICE_GENERATION.load(Ordering::Relaxed);
        if generation == self.last_generation {
            return Vec::new();
        }
        self.last_generation = generation;

        let current: Vec<(u32, String)> = hal::enumerate_output_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();

        let mut changes = Vec::new();
        for (id, name) in &current {
            if !self.baseline.iter().any(|(base_id, _)| base_id == id) {
                changes.push(RouteChange::DeviceAdded(name.clone()));
            }
        }
        for (id, name) in &self.baseline {
            if *id != 0 && !current.iter().any(|(cur_id, _)| cur_id == id) {
                changes.push(RouteChange::DeviceRemoved(name.clone()));
            }
        }
        self.baseline = current;
        changes
    }
}
