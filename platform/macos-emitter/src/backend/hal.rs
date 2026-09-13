//! Core Audio HAL output-device metadata (macOS).
//!
//! Pure property reads on `AudioObject` graph — **off the data path** (run at
//! setup / on the worker when a hotplug generation bumps). The exact FFI
//! sequences in this module were **live-validated on this host** during
//! development: enumeration returned 3 devices, name/transport reads returned
//! real values ("Mac mini Speakers", transport `0x626c746e` = `'bltn'`
//! built-in). See `build-check.md` for the annotated run + evidence.
//!
//! RT discipline: none of these are called on the capture callback; the
//! callback only memcpys bytes (see `system_capture`).

use core_foundation::base::TCFType;
use coreaudio_sys::*;

use crate::Error;

/// One enumerated HAL output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// Core Audio `AudioObjectID`.
    pub id: u32,
    /// Device name (`kAudioObjectPropertyName`, via CFString).
    pub name: String,
    /// True when the device reports the USB transport
    /// (`kAudioDeviceTransportTypeUSB`). Not authoritative for every
    /// class-compliant DAC (some report a different transport; PLATFORM_MATRIX
    /// USB-DAC row — see build-check.md).
    pub is_usb: bool,
    /// True when this device is the current system default output
    /// (`kAudioHardwarePropertyDefaultOutputDevice`).
    pub is_default: bool,
}

/// Raw `kAudioDeviceTransportTypeUSB` four-char code ('usb ' = 0x75736220).
pub const USB_TRANSPORT: u32 = kAudioDeviceTransportTypeUSB;

/// `true` when the transport four-char code is the USB transport.
pub fn transport_is_usb(transport: u32) -> bool {
    transport == USB_TRANSPORT
}

/// All HAL `AudioObjectID`s (`kAudioHardwarePropertyDevices`).
pub fn all_device_ids() -> Result<Vec<u32>, Error> {
    let addr = global(kAudioHardwarePropertyDevices);
    let mut size: u32 = 0;
    let st = unsafe {
        AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject,
            &addr,
            0,
            core::ptr::null(),
            &mut size,
        )
    };
    if st != 0 {
        return Err(hal_err("kAudioHardwarePropertyDevices size", st));
    }
    let count = size as usize / std::mem::size_of::<u32>();
    let mut ids = vec![0u32; count];
    if count > 0 {
        let st = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &addr,
                0,
                core::ptr::null(),
                &mut size,
                ids.as_mut_ptr().cast(),
            )
        };
        if st != 0 {
            return Err(hal_err("kAudioHardwarePropertyDevices data", st));
        }
    }
    Ok(ids)
}

/// The current system default output device id, or `None` (0 = none/unknown).
pub fn default_output_device_id() -> Result<Option<u32>, Error> {
    let addr = global(kAudioHardwarePropertyDefaultOutputDevice);
    let mut id: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let st = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &addr,
            0,
            core::ptr::null(),
            &mut size,
            (&mut id as *mut u32).cast(),
        )
    };
    if st != 0 {
        return Err(hal_err("kAudioHardwarePropertyDefaultOutputDevice", st));
    }
    Ok(if id == 0 { None } else { Some(id) })
}

/// Whether the device has at least one output stream (output-scope
/// `kAudioDevicePropertyStreams` non-empty). Used to filter input-only /
/// aggregate HAL objects out of the output-device list.
pub fn output_capable(id: u32) -> Result<bool, Error> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreams,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size: u32 = 0;
    let st = unsafe { AudioObjectGetPropertyDataSize(id, &addr, 0, core::ptr::null(), &mut size) };
    if st != 0 {
        return Err(hal_err("kAudioDevicePropertyStreams (output scope)", st));
    }
    Ok(size > 0)
}

/// Human-readable device name (`kAudioObjectPropertyName` → CFString → UTF-8).
pub fn device_name(id: u32) -> Result<String, Error> {
    let addr = global(kAudioObjectPropertyName);
    let mut cfs: *const core::ffi::c_void = core::ptr::null();
    let mut size: u32 = std::mem::size_of::<*const core::ffi::c_void>() as u32;
    let st = unsafe {
        AudioObjectGetPropertyData(
            id,
            &addr,
            0,
            core::ptr::null(),
            &mut size,
            (&mut cfs as *mut *const core::ffi::c_void).cast(),
        )
    };
    if st != 0 {
        return Err(hal_err("kAudioObjectPropertyName", st));
    }
    if cfs.is_null() {
        return Err(Error::MacAudio(
            "kAudioObjectPropertyName returned null".into(),
        ));
    }
    // Wrap the retained CFString under a create rule so it is released on
    // drop. We bridge through *const c_void because coreaudio-sys and
    // core-foundation each define their own opaque `__CFString`.
    let cfstr = unsafe {
        core_foundation::string::CFString::wrap_under_create_rule(
            cfs as core_foundation::string::CFStringRef,
        )
    };
    Ok(cfstr.to_string())
}

/// The device's transport four-char code (`kAudioDevicePropertyTransportType`).
pub fn device_transport_type(id: u32) -> Result<u32, Error> {
    let addr = global(kAudioDevicePropertyTransportType);
    let mut transport: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let st = unsafe {
        AudioObjectGetPropertyData(
            id,
            &addr,
            0,
            core::ptr::null(),
            &mut size,
            (&mut transport as *mut u32).cast(),
        )
    };
    if st != 0 {
        return Err(hal_err("kAudioDevicePropertyTransportType", st));
    }
    Ok(transport)
}

/// Enumerate output-capable devices with name / USB / default flags.
///
/// Off the data path (setup / route-change worker). Errors on individual
/// devices (rare property read failures) are tolerated — a device with an
/// unreadable name or transport is still listed with best-effort values.
pub fn enumerate_output_devices() -> Result<Vec<OutputDevice>, Error> {
    let default_id = default_output_device_id()?;
    let mut out = Vec::new();
    for id in all_device_ids()? {
        if !output_capable(id).unwrap_or(false) {
            continue;
        }
        let name = device_name(id).unwrap_or_else(|_| format!("HAL device {id}"));
        let transport = device_transport_type(id).unwrap_or(0);
        out.push(OutputDevice {
            id,
            name,
            is_usb: transport_is_usb(transport),
            is_default: Some(id) == default_id,
        });
    }
    Ok(out)
}

/// Global-scope property address on `kAudioObjectPropertyElementMain`.
fn global(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn hal_err(what: &str, status: i32) -> Error {
    Error::MacAudio(format!("{what}: OSStatus {status}"))
}
