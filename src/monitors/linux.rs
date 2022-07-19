use core::slice;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{env, mem, ptr};

use image::RgbaImage;
use lru::LruCache;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use x11::{xinerama, xlib};

use crate::directories::ids::WallpaperID;
use crate::wallpaper::OPTIMISTIC_CACHE;

static IS_X: AtomicBool = AtomicBool::new(false);


#[derive(Debug)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    top: i16,
    left: i16,
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

        let mut num: i32 = 0;
        let screen_info = xinerama::XineramaQueryScreens(dpy, ptr::addr_of_mut!(num));
        if screen_info.is_null() || num <= 0 {
            println!("Failed list screens in X session {:?}", display);
            if !screen_info.is_null() {
                xlib::XFree(screen_info as *mut c_void);
            }
            xlib::XCloseDisplay(dpy);
            return Vec::new();
        }

        let monitors = slice::from_raw_parts(screen_info, num as usize)
            .iter()
            .enumerate()
            .map(|(index, si)| Monitor {
                width: si.width as u32,
                height: si.height as u32,
                top: si.y_org,
                left: si.x_org,
                index,
            })
            .collect();

        xlib::XFree(screen_info as *mut c_void);
        xlib::XCloseDisplay(dpy);

        monitors
    }
}

// Set to true when we can bypass writing files to disk
// This isn't always faster (if comparing two similar settings back and forth).
pub fn supports_memory_papers() -> bool {
    IS_X.load(Ordering::Relaxed)
}

fn create_x_image(img: &RgbaImage, dpy: *mut xlib::_XDisplay) -> *mut xlib::XImage {
    let (w, h) = img.dimensions();
    let raw_img = img.as_raw();
    let size = mem::size_of_val(&raw_img[0]) * raw_img.len();

    unsafe {
        use xlib::*;

        let buf = libc::malloc(size) as *mut i8;
        assert!(!buf.is_null(), "Failed to allocate image buffer.");
        buf.copy_from_nonoverlapping(raw_img.as_ptr() as *const i8, size);


        XCreateImage(dpy, CopyFromParent as *mut Visual, 24, ZPixmap, 0, buf, w, h, 32, 0)
    }
}

// This part was figured out from reading several examples online including feh.
fn set_x_atoms(dpy: *mut xlib::_XDisplay, root: u64, pm: u64) {
    let xrootmap = CString::new("_XROOTPMAP_ID").unwrap();
    let esetrootmap = CString::new("ESETROOT_PMAP_ID").unwrap();

    unsafe {
        use xlib::*;

        let root_atom = XInternAtom(dpy, xrootmap.as_ptr(), 1);
        let esetroot_atom = XInternAtom(dpy, esetrootmap.as_ptr(), 1);

        if root_atom != 0 && esetroot_atom != 0 {
            let mut prop_type = 0;
            let mut data_root = ptr::null_mut();
            let mut length = 0;
            let mut format = 0;
            let mut after = 0;
            XGetWindowProperty(
                dpy,
                root,
                root_atom,
                0,
                1,
                0,
                AnyPropertyType as u64,
                &mut prop_type,
                &mut format,
                &mut length,
                &mut after,
                &mut data_root,
            );

            if prop_type == XA_PIXMAP {
                let mut data_esetroot = ptr::null_mut();

                XGetWindowProperty(
                    dpy,
                    root,
                    esetroot_atom,
                    0,
                    1,
                    0,
                    AnyPropertyType as u64,
                    &mut prop_type,
                    &mut format,
                    &mut length,
                    &mut after,
                    &mut data_esetroot,
                );

                if !data_root.is_null()
                    && !data_esetroot.is_null()
                    && prop_type == XA_PIXMAP
                    && *(data_root as *const Pixmap) == *(data_esetroot as *const Pixmap)
                {
                    XKillClient(dpy, *(data_root as *const Pixmap));
                }

                if !data_esetroot.is_null() {
                    XFree(data_esetroot as *mut libc::c_void);
                }
            }

            if !data_root.is_null() {
                XFree(data_root as *mut libc::c_void);
            }
        }

        let root_atom = XInternAtom(dpy, xrootmap.as_ptr(), 0);
        let esetroot_atom = XInternAtom(dpy, esetrootmap.as_ptr(), 0);

        if root_atom == 0 || esetroot_atom == 0 {
            println!("Failed to set X atoms");
            return;
        }

        XChangeProperty(
            dpy,
            root,
            root_atom,
            XA_PIXMAP,
            32,
            PropModeReplace,
            ptr::addr_of!(pm) as *const u8,
            1,
        );
        XChangeProperty(
            dpy,
            root,
            esetroot_atom,
            XA_PIXMAP,
            32,
            PropModeReplace,
            ptr::addr_of!(pm) as *const u8,
            1,
        );
    }
}

fn set_x_wallpapers(
    wallpapers: &[(&&impl WallpaperID, &Monitor)],
    cache: &LruCache<PathBuf, RgbaImage>,
) {
    let display = if let Ok(d) = env::var("DISPLAY") {
        d
    } else {
        println!("No DISPLAY set");
        return;
    };

    let display = CString::new(display).unwrap();


    // Load all uncached wallpapers and convert each one into an XImage.
    let unloaded: HashSet<_> = wallpapers
        .iter()
        .filter_map(|(w, m)| {
            let p = w.cached_abs_path(m, &w.get_props(m));
            (!cache.contains(&p)).then_some(p)
        })
        .collect();
    let loaded_map: HashMap<_, _> = unloaded
        .into_par_iter()
        .map(|p| {
            let mut img = image::open(&p).unwrap().into_rgba8();
            img.chunks_exact_mut(4).for_each(|c| c.swap(0, 2));
            (p, img)
        })
        .collect();

    let get_bgra = |p: &PathBuf| cache.peek(p).unwrap_or_else(|| loaded_map.get(p).unwrap());

    // We do not free the contents, but do free the images
    unsafe {
        use xlib::*;

        let dpy = XOpenDisplay(display.as_ptr());
        assert!(!dpy.is_null(), "Failed to open X session {:?}", display);

        let screen = XDefaultScreen(dpy);
        let (sw, sh) = (XDisplayWidth(dpy, screen), XDisplayHeight(dpy, screen));
        let root = XRootWindow(dpy, screen);

        let mut count = 0;
        let depths = XListDepths(dpy, screen, &mut count);
        let has_24 = !depths.is_null()
            && count > 0
            && slice::from_raw_parts(depths, count as usize).contains(&24);
        if !depths.is_null() {
            XFree(depths as *mut c_void);
        }

        if !has_24 {
            XCloseDisplay(dpy);
            panic!("Could not get desired depth of 24");
        }
        let depth = 24;


        XSync(dpy, 0);

        // Black rectangle is probably unnecessary, but so cheap it's fine as a failsafe.
        let pm = XCreatePixmap(dpy, root, sw as u32, sh as u32, depth as u32);
        let gc = XCreateGC(dpy, pm, 0, ptr::null_mut());
        XSetForeground(dpy, gc, XBlackPixel(dpy, screen));
        XFillRectangle(dpy, pm, gc, 0, 0, sw as u32, sh as u32);


        let mut ximgs = HashMap::new();

        for (w, m) in wallpapers {
            let p = w.cached_abs_path(m, &w.get_props(m));
            let ximg = match ximgs.entry(p) {
                Entry::Occupied(o) => *o.into_mut(),
                Entry::Vacant(v) => {
                    let ximg = create_x_image(get_bgra(v.key()), dpy);
                    *v.insert(ximg)
                }
            };

            XPutImage(
                dpy,
                pm,
                gc,
                ximg,
                0,
                0,
                m.left as i32,
                m.top as i32,
                (*ximg).width as u32,
                (*ximg).height as u32,
            );
        }

        ximgs.into_values().for_each(|ximg| {
            XDestroyImage(ximg);
        });

        set_x_atoms(dpy, root, pm);

        XSetWindowBackgroundPixmap(dpy, root, pm);
        XClearWindow(dpy, root);
        XFlush(dpy);
        XFreeGC(dpy, gc);
        XSetCloseDownMode(dpy, RetainPermanent);

        XCloseDisplay(dpy);
    }
}

pub fn set_wallpapers(wallpapers: &[(&impl WallpaperID, &[Monitor])], temp: bool) {
    let mut x: Vec<_> = wallpapers
        .iter()
        .flat_map(move |(wid, ms)| ms.iter().map(move |m| (wid, m)))
        .collect();

    x.sort_by_key(|(_, m)| m.index);

    if IS_X.load(Ordering::Relaxed) {
        if let Some(cache) = OPTIMISTIC_CACHE.get() {
            set_x_wallpapers(&x, &*cache.lock().unwrap());
        } else {
            set_x_wallpapers(&x, &LruCache::new(0));
        }

        if !temp {
            // TODO -- write fehbg or a similar restore file
            // TODO -- implement "wallpapers restore" or a more general "wallpapers set"
        }
    } else {
        todo!()
    }
}
