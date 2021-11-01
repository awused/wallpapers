use core::slice;
use std::env;
use std::ffi::{c_void, CString};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use x11::{xinerama, xlib};

use crate::directories::ids::WallpaperID;

static IS_X: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    pub index: usize,
}

pub fn list() -> Vec<Monitor> {
    let display = if let Ok(d) = env::var("DISPLAY") {
        d
    } else {
        println!("No DISPLAY set");
        return Vec::new();
    };

    let display = CString::new(display).unwrap();

    // TODO -- wayland/sway here.

    IS_X.store(true, Ordering::Relaxed);

    unsafe {
        let dpy = xlib::XOpenDisplay(display.as_ptr());
        if dpy.is_null() {
            println!("Failed to open X session {:?}", display);
            return Vec::new();
        }

        let mut num: i32 = 4;
        let screen_info = xinerama::XineramaQueryScreens(dpy, &mut num as *mut i32);
        if screen_info.is_null() || num <= 0 {
            println!("Failed list screens in X session {:?}", display);
            xlib::XFree(screen_info as *mut c_void);
            return Vec::new();
        }

        let monitors = slice::from_raw_parts(screen_info, num as usize)
            .iter()
            .enumerate()
            .map(|(index, si)| Monitor {
                width: si.width as u32,
                height: si.height as u32,
                index,
            })
            .collect();

        xlib::XFree(screen_info as *mut c_void);
        xlib::XCloseDisplay(dpy);

        monitors
    }
}

pub fn set_wallpapers(wallpapers: Vec<(impl WallpaperID, Vec<Monitor>)>, temp: bool) {
    let mut x: Vec<_> = wallpapers
        .iter()
        .map(move |(wid, ms)| ms.iter().map(move |m| (wid, m)))
        .flatten()
        .collect();

    x.sort_by_key(|(_, m)| m.index);

    // At the end of the day feh is just more practical and is a reasonable dependency
    if IS_X.load(Ordering::Relaxed) {
        let mut cmd = Command::new("feh");
        cmd.arg("--bg-center")
            .args(x.iter().map(|(w, m)| w.cached_abs_path(m, &w.get_props(m))));
        if temp {
            cmd.arg("--no-fehbg");
        }

        cmd.output()
            .expect("Error setting wallpapers for X session");
    } else {
        todo!()
    }
}
