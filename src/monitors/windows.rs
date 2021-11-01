use std::ffi::c_void;
use std::{io, ptr};

use widestring::U16CString;
use winapi::shared::windef::RECT;
use winapi::shared::winerror::E_FAIL;
use winapi::shared::wtypesbase::CLSCTX_LOCAL_SERVER;
use winapi::um::combaseapi::{CoCreateInstance, CoTaskMemFree, CoUninitialize};
use winapi::um::objbase::CoInitialize;
use winapi::um::shobjidl_core::{CLSID_DesktopWallpaper, IDesktopWallpaper};
use winapi::Interface;

use crate::directories::ids::WallpaperID;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    // pub left: i32,
    // pub top: i32,
    pub path: U16CString,
}

// Just return an empty monitors list rather than panicking
fn check(result: i32) -> Result<(), Vec<Monitor>> {
    if result == 0 {
        Ok(())
    } else {
        println!("{:?}", io::Error::from_raw_os_error(result));
        Err(Vec::new())
    }
}

unsafe fn get_monitor(dtop: &IDesktopWallpaper, n: u32) -> Result<Option<Monitor>, Vec<Monitor>> {
    let mut monitor_id: *mut u16 = ptr::null_mut();
    check(dtop.GetMonitorDevicePathAt(n, &mut monitor_id))?;
    assert!(!monitor_id.is_null());

    let path = U16CString::from_ptr_str(monitor_id);

    // We can free the memory Windows allocated immediately.
    CoTaskMemFree(monitor_id as *mut c_void);

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let result = dtop.GetMonitorRECT(path.as_ptr(), &mut rect);
    if result != E_FAIL {
        check(result)?;
    } else {
        // Sometimes windows hallucinates monitors. Hell if I know. Ignore E_FAIL failures here.
        return Ok(None);
    }

    Ok(Some(Monitor {
        width: (rect.right - rect.left) as u32,
        height: (rect.bottom - rect.top) as u32,
        // left: rect.left,
        // top: rect.top,
        path,
    }))
}


// In error cases this can leak but we'll be closing the program anyway.
pub fn list() -> Vec<Monitor> {
    let monitors = (|| unsafe {
        check(CoInitialize(ptr::null_mut()))?;

        let mut monitors = Vec::new();

        let mut desktop: *mut IDesktopWallpaper = ptr::null_mut();

        check(CoCreateInstance(
            &CLSID_DesktopWallpaper,
            ptr::null_mut(),
            CLSCTX_LOCAL_SERVER,
            &IDesktopWallpaper::uuidof(),
            &mut desktop as *mut *mut IDesktopWallpaper as *mut *mut c_void,
        ))?;

        let dtop = desktop.as_ref().unwrap();

        let mut monitor_count: u32 = 0;
        check(dtop.GetMonitorDevicePathCount(&mut monitor_count as *mut u32))?;

        for n in 0..monitor_count {
            if let Some(m) = get_monitor(dtop, n)? {
                monitors.push(m);
            }
        }


        dtop.Release();
        CoUninitialize();
        Ok(monitors)
    })();

    match monitors {
        Ok(v) | Err(v) => v,
    }
}

pub fn set_wallpapers(wallpapers: Vec<(impl WallpaperID, Vec<Monitor>)>, temp: bool) {
    let mut x: Vec<_> = wallpapers
        .iter()
        .map(move |(wid, ms)| ms.iter().map(move |m| (wid, m)))
        .flatten()
        .collect();

    todo!()
}
