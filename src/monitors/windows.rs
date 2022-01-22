use std::ffi::c_void;
use std::time::Duration;
use std::{io, ptr, thread};

use widestring::U16CString;
use winapi::shared::windef::RECT;
use winapi::shared::winerror::E_FAIL;
use winapi::shared::wtypesbase::CLSCTX_LOCAL_SERVER;
use winapi::um::combaseapi::{CoCreateInstance, CoTaskMemFree, CoUninitialize};
use winapi::um::objbase::CoInitialize;
use winapi::um::shobjidl_core::{CLSID_DesktopWallpaper, IDesktopWallpaper, DWPOS_CENTER};
use winapi::Interface;

use crate::directories::ids::WallpaperID;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    pub path: U16CString,
}

// Just return an empty monitors list rather than panicking
fn check(result: i32) -> Result<(), io::Error> {
    if result == 0 {
        Ok(())
    } else {
        let e = io::Error::from_raw_os_error(result);
        println!("{:?}", io::Error::from_raw_os_error(result));
        Err(e)
    }
}

unsafe fn get_monitor(dtop: &IDesktopWallpaper, n: u32) -> Result<Option<Monitor>, io::Error> {
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
        path,
    }))
}


// In error cases this can leak but we'll be closing the program anyway.
pub fn list() -> Vec<Monitor> {
    let monitors: Result<_, io::Error> = (|| unsafe {
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
        Ok(v) => v,
        Err(_) => Vec::new(),
    }
}

pub fn set_wallpapers(wallpapers: &[(&impl WallpaperID, &[Monitor])], _temp: bool) {
    // TODO -- maybe set legacy registry keys. Likely useless but I want to be sure.

    let wallmons: Vec<_> = wallpapers
        .iter()
        .flat_map(move |(wid, ms)| ms.iter().map(move |m| (wid, m)))
        .collect();

    let r: Result<_, io::Error> = (|| unsafe {
        check(CoInitialize(ptr::null_mut()))?;

        let mut desktop: *mut IDesktopWallpaper = ptr::null_mut();

        check(CoCreateInstance(
            &CLSID_DesktopWallpaper,
            ptr::null_mut(),
            CLSCTX_LOCAL_SERVER,
            &IDesktopWallpaper::uuidof(),
            &mut desktop as *mut *mut IDesktopWallpaper as *mut *mut c_void,
        ))?;

        let dtop = desktop.as_ref().unwrap();

        check(dtop.SetPosition(DWPOS_CENTER))?;

        for (wid, m) in wallmons {
            let u16_path = U16CString::from_os_str(wid.cached_abs_path(m, &wid.get_props(m)))
                .expect("Invalid wallpaper path containing null");
            check(dtop.SetWallpaper(m.path.as_ptr(), u16_path.as_ptr()))?;
        }

        dtop.Release();
        CoUninitialize();
        Ok(())
    })();
    drop(r);

    // If the temporary files are cleaned up too fast Windows will fail to change the wallpaper.
    // 5 seconds is more than enough time for Windows to finish or fail.
    thread::sleep(Duration::from_secs(5));
}
