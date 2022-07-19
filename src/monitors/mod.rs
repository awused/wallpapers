#[cfg(unix)]
mod linux;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;

#[cfg(unix)]
pub use linux::*;
#[cfg(windows)]
pub use windows::*;

use crate::config::CONFIG;


impl Monitor {
    pub fn cache_dir(&self) -> PathBuf {
        let monres = self.width.to_string() + "x" + &self.height.to_string();
        CONFIG.cache_directory.join(monres)
    }
}
