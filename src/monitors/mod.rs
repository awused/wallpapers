#[cfg(unix)]
mod linux;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;

#[cfg(unix)]
pub use linux::{list, set_wallpapers, Monitor};
#[cfg(windows)]
pub use windows::{list, set_wallpapers, Monitor};

use crate::config::CONFIG;


impl Monitor {
    pub fn cache_dir(&self) -> PathBuf {
        let monres = self.width.to_string() + "x" + &self.height.to_string();
        CONFIG.cache_directory.join(monres)
    }
}
