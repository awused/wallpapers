use std::time::Duration;
use std::{io, thread};

use widestring::U16CString;
use windows::core::PCWSTR;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitialize, CoTaskMemFree, CoUninitialize, CLSCTX_LOCAL_SERVER,
};
use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper, DWPOS_CENTER};

use crate::directories::ids::WallpaperID;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    pub path: U16CString,
}

// Just return an empty monitors list rather than panicking
unsafe fn get_monitor(dtop: &IDesktopWallpaper, n: u32) -> Result<Option<Monitor>, io::Error> {
    unsafe {
        let monitor_id = dtop.GetMonitorDevicePathAt(n)?;

        assert!(!monitor_id.is_null());

        let path = U16CString::from_ptr_str(monitor_id.as_ptr());

        // We can free the memory Windows allocated immediately.
        CoTaskMemFree(Some(monitor_id.as_ptr() as _));

        let result = dtop.GetMonitorRECT(PCWSTR(path.as_ptr()));

        if let Err(e) = &result {
            if e.code() == E_FAIL {
                // Sometimes windows hallucinates monitors. Hell if I know. Ignore E_FAIL failures
                // here.
                return Ok(None);
            }
        }

        let rect = result?;

        Ok(Some(Monitor {
            width: (rect.right - rect.left) as u32,
            height: (rect.bottom - rect.top) as u32,
            path,
        }))
    }
}


pub fn supports_memory_papers() -> bool {
    false
}

// In error cases this can leak but we'll be closing the program anyway.
pub fn list() -> Vec<Monitor> {
    let monitors: Result<_, io::Error> = (|| unsafe {
        CoInitialize(None)?;

        let mut monitors = Vec::new();

        let desktop: IDesktopWallpaper =
            CoCreateInstance(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER)?;

        let monitor_count = desktop.GetMonitorDevicePathCount()?;

        for n in 0..monitor_count {
            if let Some(m) = get_monitor(&desktop, n)? {
                monitors.push(m);
            }
        }

        drop(desktop);
        CoUninitialize();
        Ok(monitors)
    })();

    match monitors {
        Ok(v) => v,
        Err(e) => {
            println!("Error getting list of monitors: {e}");
            Vec::new()
        }
    }
}

pub fn set_wallpapers(wallpapers: &[(&impl WallpaperID, &[Monitor])], _temp: bool) {
    // TODO -- maybe set legacy registry keys. Likely useless but I want to be sure.

    let wallmons: Vec<_> = wallpapers
        .iter()
        .flat_map(move |(wid, ms)| ms.iter().map(move |m| (wid, m)))
        .collect();

    let r: Result<_, io::Error> = (|| unsafe {
        CoInitialize(None)?;

        let desktop: IDesktopWallpaper =
            CoCreateInstance(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER)?;


        desktop.SetPosition(DWPOS_CENTER)?;

        for (wid, m) in wallmons {
            let u16_path = U16CString::from_os_str(wid.cached_abs_path(m, &wid.get_props(m)))
                .expect("Invalid wallpaper path containing null");
            desktop.SetWallpaper(PCWSTR(m.path.as_ptr()), PCWSTR(u16_path.as_ptr()))?;
        }

        drop(desktop);
        CoUninitialize();
        Ok(())
    })();
    if let Err(e) = r {
        println!("{e}");
    }

    // If the temporary files are cleaned up too fast Windows will fail to change the wallpaper.
    // 5 seconds is more than enough time for Windows to finish or fail.
    thread::sleep(Duration::from_secs(5));
}
