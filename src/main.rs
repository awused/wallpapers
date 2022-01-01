#![cfg_attr(feature = "windows-quiet", windows_subsystem = "windows")]

use std::collections::{BTreeMap, HashSet};
use std::fs::{remove_dir, remove_file};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use aw_shuffle::persistent::rocksdb::Shuffler;
use aw_shuffle::persistent::{Options, PersistentShuffler};
use aw_shuffle::AwShuffler;
use clap::StructOpt;
use config::{string_to_colour, ImageProperties, PROPERTIES};
use crossbeam_utils::thread::scope;
use directories::ids::{TempWallpaperID, WallpaperID, TEMP_PROPS};
use monitors::Monitor;
use once_cell::sync::Lazy;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tempfile::TempDir;
use walkdir::{DirEntry, WalkDir};

use crate::config::CONFIG;
use crate::directories::get_all_originals;
use crate::wallpaper::Wallpaper;

pub(crate) mod closing;
mod config;
mod directories;
mod interactive;
pub(crate) mod monitors;
pub(crate) mod processing;
mod wallpaper;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "wallpapers",
    about = "Tool for managing and shuffling a large number of wallpapers"
)]
pub struct Opt {
    #[structopt(short, long, parse(from_os_str))]
    /// Override the selected config.
    awconf: Option<PathBuf>,

    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(Debug, StructOpt)]
enum Command {
    /// Display a random wallpaper on each monitor.
    Random,
    /// Prepopulate the cache of stale files and remove stale files.
    Sync {
        /// Also remove all wallpapers for resolutions that don't match any current monitors.
        /// For example, if you used to have a 1080p monitor but got rid of it, this can clean up
        /// unnecessary files.
        #[structopt(long)]
        clean_monitors: bool,
    },
    /// Preview a single wallpaper on every monitor.
    Preview {
        #[structopt(short, long, allow_hyphen_values = true)]
        /// Vertical offset as a percentage. Positive values translate the viewport upwards.
        vertical: Option<f64>,

        #[structopt(short, long, allow_hyphen_values = true)]
        /// Horizontal offset as a percentage. Positive values translate the viewport to the right.
        horizontal: Option<f64>,

        #[structopt(short, long, allow_hyphen_values = true)]
        /// Rows to crop off the top, negative values pad.
        top: Option<i32>,

        #[structopt(short, long, allow_hyphen_values = true)]
        /// Rows to crop off the bottom, negative values pad.
        bottom: Option<i32>,

        #[structopt(short, long, allow_hyphen_values = true)]
        /// Columns to crop off the left, negative values pad.
        left: Option<i32>,

        #[structopt(short, long, allow_hyphen_values = true)]
        /// Columns to crop off the right, negative values pad.
        right: Option<i32>,

        #[structopt(long = "bg")]
        /// Background colour to use when padding. Black, white, or an RRGGBB hex string. Example:
        /// a1b2c3
        background: Option<String>,

        #[structopt(short, long, allow_hyphen_values = true)]
        /// Level of denoising to apply. The exact specifics depend on the upscaler being used.
        /// Defaults to 1.
        denoise: Option<i32>,

        #[structopt(parse(from_os_str))]
        file: PathBuf,
    },
    Interactive {
        #[structopt(parse(from_os_str))]
        file: PathBuf,
    },
}

pub static OPTIONS: Lazy<Opt> = Lazy::new(Opt::parse);

fn main() {
    closing::init();

    match &OPTIONS.cmd {
        Command::Random {} => random(),
        Command::Sync { clean_monitors } => sync(*clean_monitors),
        Command::Preview {
            vertical,
            horizontal,
            top,
            bottom,
            left,
            right,
            background,
            denoise,
            file,
        } => {
            let mut props = TEMP_PROPS.write().unwrap();
            *props = ImageProperties {
                vertical: *vertical,
                horizontal: *horizontal,
                top: *top,
                bottom: *bottom,
                left: *left,
                right: *right,
                background: background.as_ref().map(|s| {
                    string_to_colour(s).unwrap_or_else(|| panic!("Couldn't parse colour {}", s))
                }),
                denoise: *denoise,
                nested: BTreeMap::new(),
            };
            drop(props);

            preview(file)
        }
        Command::Interactive { file } => {
            interactive::run(file);
        }
    }
}

fn random() {
    let tdir = make_tdir();

    let monitors = monitors::list();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    let wallpapers = get_all_originals().unwrap();
    if wallpapers.is_empty() {
        println!("No wallpapers found");
        return;
    }

    let options = Options::default().keep_unrecognized(true);
    let mut shuffler = Shuffler::new(&CONFIG.database, options, Some(wallpapers)).unwrap();

    let selection = shuffler.try_unique_n(monitors.len()).unwrap();
    let selection: Vec<_> = selection
        .expect("Impossible")
        .iter()
        .map(|wid| (*wid).clone())
        .collect();

    // Merge any duplicate wallpapers.
    let mut wids = Vec::new();
    let mut grouped_monitors: Vec<Vec<_>> = Vec::new();

    // O(n^2) but the real number of monitors will always be tiny
    'outer: for (wid, m) in selection.into_iter().zip(monitors.into_iter()) {
        for (i, w) in wids.iter().enumerate() {
            if wid == *w {
                grouped_monitors[i].push(m);
                continue 'outer;
            }
        }

        wids.push(wid);
        grouped_monitors.push(vec![m]);
    }

    if closing::closed() {
        return;
    }

    assert_eq!(wids.len(), grouped_monitors.len());
    let combined: Vec<_> = wids
        .iter()
        .zip(grouped_monitors.iter().map(Vec::as_slice))
        .collect();


    combined.par_iter().for_each(|(wid, monitors)| {
        Wallpaper::new(*wid, *monitors, &tdir).process(true);
    });


    if !closing::closed() {
        monitors::set_wallpapers(combined.as_slice(), false);
    }

    // We could close the shuffler earlier but this acts as a de-facto lock preventing other
    // instances from running.
    shuffler.close().unwrap();
}

fn sync(clean_monitors: bool) {
    let tdir = make_tdir();

    let monitors = monitors::list();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    let wallpapers = get_all_originals().unwrap();
    if wallpapers.is_empty() {
        println!("No wallpapers found");
        return;
    }

    let options = Options::default().keep_unrecognized(false);
    let mut shuffler = Shuffler::new(&CONFIG.database, options, Some(wallpapers.clone())).unwrap();

    shuffler.compact().unwrap();

    if closing::closed() {
        return;
    }

    // Rayon is too parallel for this, need something dumber that isn't as proactive.
    let index = AtomicUsize::new(0);
    scope(|s| {
        for _n in 0..num_cpus::get() {
            s.spawn(|_s| {
                while !closing::closed() {
                    let i = index.fetch_add(1, Ordering::Relaxed);
                    match wallpapers.get(i) {
                        Some(wid) => Wallpaper::new(wid, &monitors, &tdir).process(true),
                        None => return,
                    }
                }
            });
        }
    })
    .unwrap();

    if closing::closed() {
        return;
    }

    let valid_files: HashSet<_> = wallpapers
        .iter()
        .flat_map(|w| {
            monitors
                .iter()
                .map(|m| w.cached_abs_path(m, &w.get_props(m)))
        })
        .collect();

    let monitor_dirs: HashSet<_> = monitors.iter().map(Monitor::cache_dir).collect();

    let walk = WalkDir::new(&CONFIG.cache_directory)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    walk.into_iter()
        .map(DirEntry::into_path)
        .filter(|e| e.is_file())
        .for_each(|f| {
            if !clean_monitors && !monitor_dirs.iter().any(|p| f.starts_with(p)) {
                return;
            }

            if valid_files.contains(&f) {
                return;
            }

            assert!(f.starts_with(&CONFIG.cache_directory));

            remove_file(&f).expect("Failed to delete file");

            let mut removed = &*f;
            // Remove any empty parent directories
            while let Some(p) = removed.parent() {
                if !p.starts_with(&CONFIG.cache_directory) || p == CONFIG.cache_directory {
                    break;
                }

                if remove_dir(p).is_err() {
                    break;
                }

                removed = p;
            }
        });

    let mut props_copy = PROPERTIES.clone();
    for w in wallpapers {
        props_copy.remove(w.slash_path());
    }

    for k in props_copy.keys() {
        println!("Unmatched image property for {:?}", k);
    }

    // We could close the shuffler earlier but this acts as a de-facto lock preventing other
    // instances from running.
    shuffler.close().unwrap();
}

fn preview(path: &Path) {
    let tdir = make_tdir();

    let monitors = monitors::list();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    let wid = TempWallpaperID::new(path, &tdir);
    let w = Wallpaper::new(&wid, &monitors, &tdir);
    w.process(false);

    if !closing::closed() {
        monitors::set_wallpapers(&[(&wid, &monitors)], true);
    }
}

fn make_tdir() -> TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("wallpapers");
    CONFIG
        .temp_dir
        .as_ref()
        .map_or_else(|| builder.tempdir(), |d| builder.tempdir_in(d))
        .expect("Error creating temporary directory.")
}
