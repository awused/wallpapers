use core::slice;
use std::collections::HashMap;
use std::ffi::CString;
use std::path::PathBuf;
use std::{env, mem, ptr};

use color_eyre::Result;
use color_eyre::eyre::bail;
use futures::stream::FuturesUnordered;
use futures::{StreamExt, TryFutureExt};
use image::RgbaImage;
use tokio::sync::oneshot;
use x11::{xinerama, xlib, xrandr};

use crate::monitors::Monitor;
use crate::processing::WORKER;
use crate::wallpaper::OPTIMISTIC_CACHE;

pub(super) fn list_monitors() -> Vec<Monitor> {
    let Ok(display) = env::var("DISPLAY") else {
        println!("No DISPLAY set");
        return Vec::new();
    };

    let display = CString::new(display).unwrap();


    unsafe {
        use xlib::*;
        use xrandr::*;

        let dpy = xlib::XOpenDisplay(display.as_ptr());
        if dpy.is_null() {
            println!("Failed to open X session {display:?}");
            return Vec::new();
        }


        let screen = XDefaultScreen(dpy);
        let root = XRootWindow(dpy, screen);

        // Xinerama is much faster, but doesn't always work. Maybe if the GPU is asleep?
        let mut num: i32 = 0;
        let xinerama_info = xinerama::XineramaQueryScreens(dpy, ptr::addr_of_mut!(num));
        if xinerama_info.is_null() || num <= 0 {
            if !xinerama_info.is_null() {
                XFree(xinerama_info.cast());
            }
        } else {
            let monitors = slice::from_raw_parts(xinerama_info, num as usize)
                .iter()
                .map(|si| Monitor {
                    width: si.width as u32,
                    height: si.height as u32,
                    top: si.y_org as i32,
                    left: si.x_org as i32,
                    name: 0,
                })
                .collect();

            XFree(xinerama_info.cast());
            XCloseDisplay(dpy);
            return monitors;
        }


        // Try XRandR as a fallback.
        let resources = XRRGetScreenResources(dpy, root);

        let mut monitors = Vec::new();
        for output in slice::from_raw_parts((*resources).outputs, (*resources).noutput as usize) {
            let info = XRRGetOutputInfo(dpy, resources, *output);

            if (*info).connection == RR_Connected as u16 {
                let crtc = XRRGetCrtcInfo(dpy, resources, (*info).crtc);
                let cinfo = &*crtc;

                monitors.push(Monitor {
                    width: cinfo.width,
                    height: cinfo.height,
                    top: cinfo.y,
                    left: cinfo.x,
                    name: 0,
                });

                XRRFreeCrtcInfo(crtc);
            }

            XRRFreeOutputInfo(info);
        }
        XRRFreeScreenResources(resources);
        XCloseDisplay(dpy);

        monitors
    }
}


#[derive(Debug)]
struct MallocedImage(*mut i8, u32, u32);
unsafe impl Send for MallocedImage {}

fn malloc_image_buf(img: &RgbaImage) -> MallocedImage {
    let (w, h) = img.dimensions();
    let raw_img = img.as_raw();
    let size = mem::size_of_val(&raw_img[0]) * raw_img.len();

    unsafe {
        let buf = libc::malloc(size).cast::<i8>();
        assert!(!buf.is_null(), "Failed to allocate image buffer.");
        buf.copy_from_nonoverlapping(raw_img.as_ptr().cast(), size);

        MallocedImage(buf, w, h)
    }
}

// This part was figured out from reading several online examples including feh.
fn set_x_atoms(xdisplay: *mut xlib::_XDisplay, root: u64, pixmap: u64) {
    let xrootmap = CString::new("_XROOTPMAP_ID").unwrap();
    let esetrootmap = CString::new("ESETROOT_PMAP_ID").unwrap();

    unsafe {
        use xlib::*;

        let root_atom = XInternAtom(xdisplay, xrootmap.as_ptr(), 1);
        let esetroot_atom = XInternAtom(xdisplay, esetrootmap.as_ptr(), 1);

        if root_atom != 0 && esetroot_atom != 0 {
            let mut prop_type = 0;
            let mut data_root = ptr::null_mut();
            let mut length = 0;
            let mut format = 0;
            let mut after = 0;
            XGetWindowProperty(
                xdisplay,
                root,
                root_atom,
                0,
                1,
                0,
                AnyPropertyType as u64,
                &raw mut prop_type,
                &raw mut format,
                &raw mut length,
                &raw mut after,
                &raw mut data_root,
            );

            if prop_type == XA_PIXMAP {
                let mut data_esetroot = ptr::null_mut();

                XGetWindowProperty(
                    xdisplay,
                    root,
                    esetroot_atom,
                    0,
                    1,
                    0,
                    AnyPropertyType as u64,
                    &raw mut prop_type,
                    &raw mut format,
                    &raw mut length,
                    &raw mut after,
                    &raw mut data_esetroot,
                );

                if !data_root.is_null()
                    && !data_esetroot.is_null()
                    && prop_type == XA_PIXMAP
                    && *(data_root as *const Pixmap) == *(data_esetroot as *const Pixmap)
                {
                    XKillClient(xdisplay, *(data_root as *const Pixmap));
                }

                if !data_esetroot.is_null() {
                    XFree(data_esetroot.cast());
                }
            }

            if !data_root.is_null() {
                XFree(data_root.cast());
            }
        }

        let root_atom = XInternAtom(xdisplay, xrootmap.as_ptr(), 0);
        let esetroot_atom = XInternAtom(xdisplay, esetrootmap.as_ptr(), 0);

        if root_atom == 0 || esetroot_atom == 0 {
            println!("Failed to set X atoms");
            return;
        }

        XChangeProperty(
            xdisplay,
            root,
            root_atom,
            XA_PIXMAP,
            32,
            PropModeReplace,
            ptr::addr_of!(pixmap).cast(),
            1,
        );
        XChangeProperty(
            xdisplay,
            root,
            esetroot_atom,
            XA_PIXMAP,
            32,
            PropModeReplace,
            ptr::addr_of!(pixmap).cast(),
            1,
        );
    }
}

pub(super) async fn set_wallpapers(wallpapers: HashMap<PathBuf, Vec<&Monitor>>) -> Result<()> {
    let Ok(display) = env::var("DISPLAY") else {
        bail!("No DISPLAY set");
    };

    let display = CString::new(display).unwrap();

    let image_futures = wallpapers.into_iter().map(|(p, ms)| {
        let (send, recv) = oneshot::channel::<MallocedImage>();

        WORKER.spawn(move || {
            let mi = if let Some(cache) = OPTIMISTIC_CACHE.get()
                && let Some(cached) = cache.read().unwrap().peek(&p)
            {
                malloc_image_buf(cached)
            } else {
                let mut img = image::open(&p).unwrap().into_rgba8();
                img.chunks_exact_mut(4).for_each(|c| c.swap(0, 2));
                malloc_image_buf(&img)
            };
            send.send(mi).unwrap();
        });

        (ms, recv)
    });

    unsafe {
        use xlib::*;

        let xdisplay = XOpenDisplay(display.as_ptr());
        assert!(!xdisplay.is_null(), "Failed to open X session {display:?}");

        let screen = XDefaultScreen(xdisplay);
        let (screen_w, screen_h) =
            (XDisplayWidth(xdisplay, screen) as u32, XDisplayHeight(xdisplay, screen) as u32);
        let root = XRootWindow(xdisplay, screen);

        let mut count = 0;
        let depths = XListDepths(xdisplay, screen, &raw mut count);
        let has_24 = !depths.is_null()
            && count > 0
            && slice::from_raw_parts(depths, count as usize).contains(&24);
        if !depths.is_null() {
            XFree(depths.cast());
        }

        if !has_24 {
            XCloseDisplay(xdisplay);
            panic!("Could not get desired depth of 24");
        }
        let depth = 24;


        XSync(xdisplay, 0);

        // Black rectangle is probably unnecessary, but so cheap it's fine as a failsafe.
        let pixmap = XCreatePixmap(xdisplay, root, screen_w, screen_h, depth as u32);
        let gc = XCreateGC(xdisplay, pixmap, 0, ptr::null_mut());
        XSetForeground(xdisplay, gc, XBlackPixel(xdisplay, screen));
        XFillRectangle(xdisplay, pixmap, gc, 0, 0, screen_w, screen_h);


        let unordered: FuturesUnordered<_> = image_futures
            .map(|(monitors, recv)| {
                recv.map_ok(|MallocedImage(buf, w, h)| {
                    // Not thread safe, but almost instant.
                    let ximg = XCreateImage(
                        xdisplay,
                        CopyFromParent as *mut Visual,
                        24,
                        ZPixmap,
                        0,
                        buf,
                        w,
                        h,
                        32,
                        0,
                    );

                    for m in monitors {
                        XPutImage(
                            xdisplay,
                            pixmap,
                            gc,
                            ximg,
                            0,
                            0,
                            m.left,
                            m.top,
                            (*ximg).width as u32,
                            (*ximg).height as u32,
                        );
                    }

                    XDestroyImage(ximg);
                })
            })
            .collect();

        // Single threaded executor, no risk of X calls from other threads.
        unordered.collect::<Vec<_>>().await;

        set_x_atoms(xdisplay, root, pixmap);

        XSetWindowBackgroundPixmap(xdisplay, root, pixmap);
        XClearWindow(xdisplay, root);
        XFlush(xdisplay);
        XFreeGC(xdisplay, gc);
        XSetCloseDownMode(xdisplay, RetainPermanent);

        XCloseDisplay(xdisplay);
        Ok(())
    }
}
