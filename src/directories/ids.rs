use std::ffi::OsString;
use std::num::NonZeroU8;
#[cfg(windows)]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::config::{ImageProperties, CONFIG, PROPERTIES};
use crate::monitors::Monitor;

pub trait WallpaperID: Send + Sync + std::fmt::Debug {
    fn original_abs_path(&self) -> PathBuf;

    fn cached_abs_path(&self, m: &Monitor, ip: &Option<ImageProperties>) -> PathBuf;

    fn get_props(&self, m: &Monitor) -> Option<ImageProperties>;

    fn cropped_rel_path(&self, ip: &Option<ImageProperties>) -> Option<PathBuf>;

    fn upscaled_rel_path(&self, scale: NonZeroU8, ip: &Option<ImageProperties>) -> PathBuf;
}


// A forward slash separated path relative to the root of the originals directory.
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct OriginalWallpaperID(PathBuf);

impl OriginalWallpaperID {
    pub(super) fn from_rel_path<P: AsRef<Path>>(p: P) -> Self {
        Self(slash_from_relative(p))
    }

    pub fn slash_path(&self) -> &Path {
        &self.0
    }
}

impl WallpaperID for OriginalWallpaperID {
    fn original_abs_path(&self) -> PathBuf {
        CONFIG.originals_directory.join(relative_from_slash(&self.0))
    }

    fn cached_abs_path(&self, m: &Monitor, ip: &Option<ImageProperties>) -> PathBuf {
        let mut p: OsString = m.cache_dir().join(relative_from_slash(&self.0)).into();

        if let Some(ip) = ip {
            let full = ip.full_string();
            if !full.is_empty() {
                p.push(full);
            }
        }

        p.push(".png");
        p.into()
    }

    fn get_props(&self, m: &Monitor) -> Option<ImageProperties> {
        let (mut a, mut b) = (m.width, m.height);

        while b != 0 {
            let c = b;
            b = a % b;
            a = c;
        }

        let a_x = (m.width / a).to_string();
        let a_y = (m.height / a).to_string();

        let props = PROPERTIES.get(&self.0)?;

        let per_monitor = props.nested.get(&a_x).and_then(|m| m.get(&a_y));
        if let Some(monprops) = per_monitor {
            Some(monprops.clone())
        } else {
            Some(props.clone())
        }
    }

    // Returns None if no cropping is necessary here.
    fn cropped_rel_path(&self, ip: &Option<ImageProperties>) -> Option<PathBuf> {
        ip.as_ref().and_then(|ip| {
            let s = ip.crop_pad_string();
            if s.is_empty() {
                return None;
            }
            let mut p: OsString =
                self.0.file_name().expect("Specified wallpaper has no filename").into();
            p.push(s);
            p.push(".png");
            Some(p.into())
        })
    }

    fn upscaled_rel_path(&self, scale: NonZeroU8, ip: &Option<ImageProperties>) -> PathBuf {
        let mut p: OsString =
            self.0.file_name().expect("Specified wallpaper has no filename").into();
        if let Some(ip) = ip.as_ref() {
            p.push(ip.crop_pad_string());
            p.push("-");
            if let Some(denoise) = ip.denoise {
                p.push(denoise.to_string());
                p.push("-")
            }
        }
        p.push(scale.to_string());
        p.push("-");
        p.push(".png");
        p.into()
    }
}


// Used for Preview/Interactive mode. An absolute path to a wallpaper that only writes things in
// the temp directory.
#[derive(Debug)]
pub struct TempWallpaperID<'a> {
    // This could safely use UnsafeCell
    path: RwLock<PathBuf>,
    // The original filename from creation time.
    // So that we don't need to waste time regenerating files if it is installed.
    fname: PathBuf,
    // This could safely use UnsafeCell
    pub props: RwLock<ImageProperties>,
    tdir: &'a TempDir,
}


impl<'a> TempWallpaperID<'a> {
    pub fn new<P: AsRef<Path>>(p: P, props: ImageProperties, tdir: &'a TempDir) -> Self {
        Self {
            path: RwLock::new(p.as_ref().to_path_buf()),
            fname: p.as_ref().file_name().expect("Wallpaper has no filename").into(),
            props: RwLock::new(props),
            tdir,
        }
    }

    // Used for install in interactive mode
    pub fn set_original_path(&self, p: PathBuf) {
        assert!(p.is_absolute());
        assert!(p.starts_with(&CONFIG.originals_directory));
        *self.path.write().unwrap() = p;
    }

    pub fn slash_path(&self) -> Option<PathBuf> {
        self.path
            .read()
            .unwrap()
            .strip_prefix(&CONFIG.originals_directory)
            .ok()
            .map(slash_from_relative)
    }
}

impl WallpaperID for TempWallpaperID<'_> {
    fn original_abs_path(&self) -> PathBuf {
        self.path.read().unwrap().clone()
    }

    fn cached_abs_path(&self, m: &Monitor, ip: &Option<ImageProperties>) -> PathBuf {
        let monres = m.width.to_string() + "x" + &m.height.to_string();
        let mut p: OsString = self.tdir.path().join(monres).join(&self.fname).into();

        if let Some(ip) = ip {
            let full = ip.full_string();
            if !full.is_empty() {
                p.push(full);
            }
        }

        p.push(".png");
        p.into()
    }

    fn get_props(&self, _m: &Monitor) -> Option<ImageProperties> {
        Some(self.props.read().unwrap().clone())
    }

    fn cropped_rel_path(&self, ip: &Option<ImageProperties>) -> Option<PathBuf> {
        ip.as_ref().and_then(|ip| {
            let s = ip.crop_pad_string();
            if s.is_empty() {
                return None;
            }
            let mut p: OsString = self.fname.clone().into();
            p.push(s);
            p.push(".png");
            Some(p.into())
        })
    }

    fn upscaled_rel_path(&self, scale: NonZeroU8, ip: &Option<ImageProperties>) -> PathBuf {
        let mut p: OsString = self.fname.clone().into();
        if let Some(ip) = ip.as_ref() {
            p.push(ip.crop_pad_string());
            p.push("-");
            if let Some(denoise) = ip.denoise {
                p.push(denoise.to_string());
                p.push("-")
            }
        }
        p.push(scale.to_string());
        p.push(".png");
        p.into()
    }
}


fn slash_from_relative<P: AsRef<Path>>(p: P) -> PathBuf {
    #[cfg(unix)]
    return p.as_ref().to_owned();
    #[cfg(windows)]
    {
        let mut pb = OsString::new();
        for c in p.as_ref().components() {
            if !pb.is_empty() {
                pb.push("/");
            }
            match c {
                Component::Normal(pc) => pb.push(pc),
                _ => unreachable!(),
            }
        }

        pb.into()
    }
}

pub fn relative_from_slash<P: AsRef<Path>>(p: P) -> PathBuf {
    #[cfg(unix)]
    return p.as_ref().to_owned();
    #[cfg(windows)]
    {
        let mut pb = PathBuf::new();
        for c in p.as_ref().components() {
            match c {
                Component::Normal(pc) => pb.push(pc),
                _ => unreachable!(),
            }
        }

        pb
    }
}
