use core::slice;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::error::Error;
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{env, future, mem, ptr};

use color_eyre::Result;
use futures::executor::block_on;
use futures::stream::FuturesUnordered;
use futures::{StreamExt, TryFutureExt};
use image::RgbaImage;
use lru::LruCache;
use tokio::sync::oneshot;
use x11::{xinerama, xlib, xrandr};

use crate::directories::ids::WallpaperID;
use crate::processing::WORKER;
use crate::wallpaper::OPTIMISTIC_CACHE;

mod wayland;
mod xorg;

static IS_WAYLAND: AtomicBool = AtomicBool::new(false);

pub const fn supports_memory_papers() -> bool {
    true
}

pub fn use_xrgb_memory() -> bool {
    IS_WAYLAND.load(Ordering::Relaxed)
}

#[allow(clippy::large_enum_variant)]
enum Kind {
    Wayland(wayland::Conn),
    X,
}

pub struct Connection(Kind);


pub fn init() -> Connection {
    if let Some(wayland) = wayland::init() {
        IS_WAYLAND.store(true, Ordering::Relaxed);
        Connection(Kind::Wayland(wayland))
    } else {
        Connection(Kind::X)
    }
}

#[derive(Debug)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    // For x11
    top: i32,
    left: i32,
    // For wayland
    name: u32,
}

impl Connection {
    pub async fn list_monitors(&mut self) -> Result<Vec<Monitor>> {
        match &mut self.0 {
            Kind::Wayland(wayland_connection) => wayland_connection.list_monitors().await,
            Kind::X => Ok(xorg::list_monitors()),
        }
    }

    pub async fn set_wallpapers(
        &mut self,
        wallpapers: &[(&impl WallpaperID, &[Monitor])],
        _temp: bool,
    ) -> Result<()> {
        let mut paths_monitors = HashMap::new();
        for (wid, ms) in wallpapers {
            for m in *ms {
                let p = wid.cached_abs_path(m, &wid.get_props(m));
                match paths_monitors.entry(p) {
                    Entry::Vacant(v) => v.insert(Vec::new()).push(m),
                    Entry::Occupied(mut e) => e.get_mut().push(m),
                }
            }
        }

        match &self.0 {
            Kind::Wayland(wayland_connection) => todo!(),
            Kind::X => {
                // Load all uncached wallpapers and convert each one into an XImage.
                xorg::set_wallpapers(paths_monitors).await;
                Ok(())
            }
        }
    }

    // Keeps any underlying connection alive and up-to-date. Will only return if the connection is
    // unexpectedly closed.
    pub async fn poll(&mut self) -> Result<()> {
        match &mut self.0 {
            Kind::Wayland(connection) => connection.poll().await,
            Kind::X => future::pending().await,
        }
    }
}
