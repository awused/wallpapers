#[cfg(windows)]
use std::ffi::OsString;
use std::io::Error;
#[cfg(windows)]
use std::path::Component;

use walkdir::{DirEntry, WalkDir};

use self::ids::OriginalWallpaperID;
use crate::config::CONFIG;

pub mod ids;

static EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "bmp"];


// Gets all the originals as forward slash separated relative paths
pub fn get_all_originals() -> Result<Vec<OriginalWallpaperID>, Error> {
    let walk = WalkDir::new(&CONFIG.originals_directory)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    Ok(walk
        .into_iter()
        .map(DirEntry::into_path)
        .filter(|p| p.is_file())
        .filter(|p| {
            if let Some(ext) = p.extension() {
                let ext = ext.to_string_lossy();
                for e in EXTENSIONS {
                    if ext.eq_ignore_ascii_case(e) {
                        return true;
                    }
                }
            }
            false
        })
        .map(|p| {
            OriginalWallpaperID::from_rel_path(
                p.strip_prefix(&CONFIG.originals_directory)
                    .expect("File in originals directory did not have correct path prefix."),
            )
        })
        .collect())
}
