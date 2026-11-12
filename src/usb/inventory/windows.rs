// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Optional, bounded reads of Windows' present serial-device location properties.
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, mem::size_of, ptr};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::*,
    Foundation::{ERROR_NO_MORE_ITEMS, GetLastError, INVALID_HANDLE_VALUE},
};

struct Devices(HDEVINFO);
impl Drop for Devices {
    fn drop(&mut self) {
        // SAFETY: this guard owns the successful SetupDiGetClassDevsW result.
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
fn property(devices: &Devices, device: &SP_DEVINFO_DATA, key: u32) -> Option<String> {
    let mut units = [0_u16; 2048];
    let mut bytes = 0;
    let mut kind = 0;
    // SAFETY: aligned initialized writable UTF-16 storage; API receives its byte
    // capacity. Device belongs to this live information set. Reads only.
    let success = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            devices.0,
            device,
            key,
            &mut kind,
            units.as_mut_ptr().cast(),
            size_of_val(&units) as u32,
            &mut bytes,
        )
    };
    if success == 0
        || bytes == 0
        || bytes as usize > size_of_val(&units)
        || !bytes.is_multiple_of(2)
    {
        return None;
    }
    // REG_SZ=1, REG_MULTI_SZ=7. Preserve multiple location paths explicitly.
    if kind != 1 && kind != 7 {
        return None;
    }
    let parts: Vec<_> = units[..bytes as usize / 2]
        .split(|u| *u == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}
pub(super) fn locations() -> Result<BTreeMap<String, String>> {
    // SAFETY: documented present-device query; no parent window, enumerator or
    // install/change flags. Its handle is owned by the guard below.
    let handle = unsafe {
        SetupDiGetClassDevsW(
            &GUID_DEVCLASS_PORTS,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT,
        )
    };
    ensure!(
        handle != INVALID_HANDLE_VALUE as isize,
        "Windows serial location query unavailable"
    );
    let devices = Devices(handle);
    let mut locations = BTreeMap::new();
    for index in 0..4096 {
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        // SAFETY: initialized structure with the documented cbSize, live set.
        if unsafe { SetupDiEnumDeviceInfo(devices.0, index, &mut device) } == 0 {
            // SAFETY: immediately observes this thread's failed API call.
            ensure!(
                unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS,
                "Windows serial location enumeration incomplete"
            );
            return Ok(locations);
        }
        let Some(friendly) = property(&devices, &device, SPDRP_FRIENDLYNAME) else {
            continue;
        };
        let Some((_, suffix)) = friendly.rsplit_once(" (COM") else {
            continue;
        };
        let Some(number) = suffix.strip_suffix(')') else {
            continue;
        };
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let name = format!("COM{number}");
        if let Some(location) = property(&devices, &device, SPDRP_LOCATION_PATHS)
            .or_else(|| property(&devices, &device, SPDRP_LOCATION_INFORMATION))
        {
            ensure!(
                locations.insert(name, location).is_none(),
                "Windows returned ambiguous serial location names"
            );
        }
    }
    anyhow::bail!("Windows serial location enumeration exceeded its limit")
}
