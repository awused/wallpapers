use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future;
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::Result;

use crate::directories::ids::WallpaperID;

mod wayland;
mod xorg;

static IS_WAYLAND: AtomicBool = AtomicBool::new(false);

pub const fn supports_memory_papers() -> bool {
    true
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
            Kind::Wayland(wcon) => wcon.list_monitors().await,
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

        match &mut self.0 {
            Kind::Wayland(wcon) => wcon.set_wallpapers(paths_monitors).await,
            Kind::X => {
                // Load all uncached wallpapers and convert each one into an XImage.
                xorg::set_wallpapers(paths_monitors).await
            }
        }
    }

    // Keeps any underlying connection alive and up-to-date. Will only return if the connection is
    // unexpectedly closed.
    pub async fn poll(&mut self) -> Result<()> {
        match &mut self.0 {
            Kind::Wayland(wcon) => wcon.poll().await,
            Kind::X => future::pending().await,
        }
    }

    pub const fn requires_persistence(&self) -> bool {
        match &self.0 {
            Kind::Wayland(_) => true,
            Kind::X => false,
        }
    }
}
