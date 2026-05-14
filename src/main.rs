#![cfg_attr(feature = "windows-quiet", windows_subsystem = "windows")]

// The tikv fork may not be easily buildable for Windows.
#[cfg(unix)]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;


use std::collections::{BTreeMap, HashSet};
use std::fs::{remove_dir, remove_file};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, RwLock};
use std::time::Duration;
use std::{process, thread};

use aw_shuffle::AwShuffler;
use aw_shuffle::persistent::rocksdb::Shuffler;
use aw_shuffle::persistent::{Options, PersistentShuffler};
use clap::Parser;
use color_eyre::Result;
use config::{ImageProperties, PROPERTIES, string_to_colour};
use crossbeam_utils::thread::scope;
use directories::ids::{TempWallpaperID, WallpaperID};
use lru::LruCache;
use monitors::Monitor;
#[cfg(feature = "opencl")]
use processing::resample::{OPENCL_QUEUE, print_gpus};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tempfile::TempDir;
use tokio::time::sleep;
use walkdir::{DirEntry, WalkDir};
use wallpaper::OPTIMISTIC_CACHE;

use crate::config::CONFIG;
use crate::directories::get_all_originals;
use crate::monitors::Connection;
use crate::processing::SMALL_POOLS;
use crate::wallpaper::Wallpaper;

pub(crate) mod closing;
mod config;
#[cfg(unix)]
mod daemon;
mod directories;
mod interactive;
pub(crate) mod monitors;
pub(crate) mod processing;
mod wallpaper;

#[derive(Debug, Parser)]
#[clap(
    name = "wallpapers",
    about = "Tool for managing and shuffling a large number of wallpapers"
)]
pub struct Opt {
    #[arg(short, long, value_parser)]
    /// Override the selected config.
    awconf: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Debug, Parser)]
enum Command {
    /// Display a random wallpaper on each monitor.
    Random,
    /// Run as a pseudo-daemon, listening for updates on SIGUSR1.
    #[cfg(unix)]
    Daemon,
    /// Prepopulate the cache of stale files and remove stale files.
    Sync {
        /// Also remove all wallpapers for resolutions that don't match any current monitors.
        /// For example, if you used to have a 1080p monitor but got rid of it, this can clean up
        /// unnecessary files.
        #[arg(long)]
        clean_monitors: bool,
    },
    /// Preview a single wallpaper on every monitor.
    Preview {
        #[arg(short, long, allow_hyphen_values = true)]
        /// Vertical offset as a percentage. Positive values translate the viewport upwards.
        vertical: Option<f64>,

        #[arg(short, long, allow_hyphen_values = true)]
        /// Horizontal offset as a percentage. Positive values translate the viewport to the right.
        horizontal: Option<f64>,

        #[arg(short, long, allow_hyphen_values = true)]
        /// Rows to crop off the top, negative values pad.
        top: Option<i32>,

        #[arg(short, long, allow_hyphen_values = true)]
        /// Rows to crop off the bottom, negative values pad.
        bottom: Option<i32>,

        #[arg(short, long, allow_hyphen_values = true)]
        /// Columns to crop off the left, negative values pad.
        left: Option<i32>,

        #[arg(short, long, allow_hyphen_values = true)]
        /// Columns to crop off the right, negative values pad.
        right: Option<i32>,

        #[arg(long = "bg")]
        /// Background colour to use when padding. Black, white, or an RRGGBB hex string. Example:
        /// a1b2c3
        background: Option<String>,

        #[arg(short, long, allow_hyphen_values = true)]
        /// Level of denoising to apply. The exact specifics depend on the upscaler being used.
        /// Defaults to 1.
        denoise: Option<i32>,

        #[arg(value_parser)]
        file: PathBuf,

        // Clap bug: https://github.com/clap-rs/clap/issues/3403
        #[arg(long)]
        /// Print help information
        help: bool,
    },
    Interactive {
        #[arg(value_parser)]
        file: PathBuf,
    },
    ListMonitors,
    #[cfg(feature = "opencl")]
    ShowGpus,
}

pub static OPTIONS: LazyLock<Opt> = LazyLock::new(Opt::parse);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    closing::init();
    color_eyre::install().unwrap();

    match &OPTIONS.cmd {
        Command::Random => random_command().await.unwrap(),
        #[cfg(unix)]
        Command::Daemon => daemon::run().await,
        Command::Sync { clean_monitors } => sync(*clean_monitors).await,
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
            help: _,
        } => {
            let props = ImageProperties {
                vertical: *vertical,
                horizontal: *horizontal,
                top: *top,
                bottom: *bottom,
                left: *left,
                right: *right,
                background: background.as_ref().map(|s| {
                    string_to_colour(s).unwrap_or_else(|| panic!("Couldn't parse colour {s}"))
                }),
                denoise: *denoise,
                nested: BTreeMap::new(),
            };

            preview(file, props).await;
        }
        Command::Interactive { file } => {
            interactive::run(file).await.unwrap();
        }
        Command::ListMonitors => print_monitors().await,
        #[cfg(feature = "opencl")]
        Command::ShowGpus => print_gpus(),
    }
}


async fn random_command() -> Result<()> {
    #[cfg(all(unix, not(feature = "x11")))]
    {
        pkill_wayland();
        return Ok(());
    }
    #[cfg(not(all(unix, not(feature = "x11"))))]
    {
        let mut con = monitors::init();
        if con.requires_persistence() {
            pkill_wayland();
            return Ok(());
        }

        let monitors = con.list_monitors().await?;
        random(&mut con, monitors).await
    }
}

fn pkill_wayland() {
    println!(
        "Random is unsupported in this environment, attempting to signal daemon by name using \
         pkill.\nPrefer calling pkill or similar directly instead."
    );
    if let Some(arg0) = std::env::args().next()
        && let Some(name) = Path::new(&arg0).file_name()
    {
        let mut c = std::process::Command::new("/usr/bin/pkill");
        // Only exact matches with a handler registered for USR1. Probably safe.
        c.arg("-x");
        c.arg("-H");
        c.arg("-USR1");
        c.arg(name);
        let mut child = c.spawn().unwrap();
        child.wait().unwrap();
    }
}

async fn random(con: &mut Connection, monitors: Vec<Monitor>) -> Result<()> {
    if monitors.is_empty() {
        println!("No monitors detected");
        return Ok(());
    }

    let tdir = LazyLock::new(make_tdir as _);


    // This will only be beneficial on cache misses, but can't hurt.
    if monitors::supports_memory_papers() {
        OPTIMISTIC_CACHE.get_or_init(|| {
            RwLock::new(LruCache::new(NonZeroUsize::new(monitors.len() * 3).unwrap()))
        });
    }

    // Opening the shuffler should only fail if it is already in use
    let mut tries = 3;
    let mut shuffler = loop {
        let wallpapers = get_all_originals()?;
        if wallpapers.is_empty() {
            println!("No wallpapers found");
            return Ok(());
        }

        let options = Options::default().keep_unrecognized(true);

        match Shuffler::new(&CONFIG.database, options, Some(wallpapers)) {
            Ok(shuffler) => break shuffler,
            Err(e) if tries == 0 => {
                return Err(e.into());
            }
            Err(e) => {
                println!("Error opening shuffler: {e}, retrying");
                // pseudo-random enough that multiple processes with the similar pids should get
                // different delays.
                let delay = process::id().reverse_bits() as u64 % 20000 + 2000;
                sleep(Duration::from_millis(delay)).await
            }
        }
        tries -= 1;
    };

    // https://github.com/rust-lang/rust-clippy/issues/9219
    let selection: Vec<_> = shuffler
        .try_unique_n(monitors.len())
        .unwrap()
        .unwrap()
        .into_iter()
        .cloned()
        .collect();
    let close_handle = thread::spawn(move || shuffler.close());


    // Merge any duplicate wallpapers.
    let mut wids = Vec::new();
    let mut grouped_monitors: Vec<Vec<_>> = Vec::new();

    // O(n^2) but the real number of monitors will always be tiny
    'outer: for (wid, m) in selection.into_iter().zip(monitors) {
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
        return Ok(());
    }

    assert_eq!(wids.len(), grouped_monitors.len());
    let combined: Vec<_> = wids.iter().zip(grouped_monitors.iter().map(Vec::as_slice)).collect();

    if !SMALL_POOLS.load(Ordering::Relaxed) {
        combined.par_iter().for_each(|(wid, monitors)| {
            Wallpaper::new(*wid, monitors, &tdir).process(true);
        });
    } else {
        combined.iter().for_each(|(wid, monitors)| {
            Wallpaper::new(*wid, monitors, &tdir).process(true);
        });
    }

    if !closing::closed() {
        con.set_wallpapers(combined.as_slice(), false).await?;
    }

    close_handle.join().unwrap()?;
    Ok(())
}

async fn sync(clean_monitors: bool) {
    let tdir = LazyLock::new(make_tdir as _);

    let mut con = monitors::init();
    let monitors = con.list_monitors().await.unwrap();
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

    // Rayon is too parallel for this, need something dumber that isn't as proactive to make the
    // order somewhat consistent.
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
        .flat_map(|w| monitors.iter().map(|m| w.cached_abs_path(m, &w.get_props(m))))
        .collect();

    let monitor_dirs: HashSet<_> = monitors.iter().map(Monitor::cache_dir).collect();

    let walk = WalkDir::new(&CONFIG.cache_directory)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    walk.into_iter().map(DirEntry::into_path).filter(|e| e.is_file()).for_each(|f| {
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

    let mut props_copy = PROPERTIES.read().unwrap().clone();
    for w in wallpapers {
        props_copy.remove(w.slash_path());
    }

    for k in props_copy.keys() {
        println!("Unmatched image property for {k:?}");
    }

    // We could close the shuffler earlier but this acts as a de-facto lock preventing other
    // instances from running.
    shuffler.close().unwrap();
}

async fn preview(path: &Path, props: ImageProperties) {
    let tdir = LazyLock::new(make_tdir as _);
    let mut con = monitors::init();


    if con.requires_persistence() {
        println!("Preview is unsupported in this environment");
        return;
    }

    let monitors = con.list_monitors().await.unwrap();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    #[cfg(feature = "opencl")]
    let cl_spawn_handle = thread::spawn(|| {
        LazyLock::force(&OPENCL_QUEUE);
    });

    if monitors::supports_memory_papers() {
        OPTIMISTIC_CACHE.get_or_init(|| {
            RwLock::new(LruCache::new(NonZeroUsize::new(monitors.len() * 3).unwrap()))
        });
    }

    let wid = TempWallpaperID::new(path, props, &tdir);
    let w = Wallpaper::new(&wid, &monitors, &tdir);
    w.process(false);

    if !closing::closed() {
        con.set_wallpapers(&[(&wid, &monitors)], true).await.unwrap();
    }

    #[cfg(feature = "opencl")]
    cl_spawn_handle.join().unwrap();
}

async fn print_monitors() {
    let mut con = monitors::init();
    let monitors = con.list_monitors().await.unwrap();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    for m in monitors {
        println!("{m:?}");
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
