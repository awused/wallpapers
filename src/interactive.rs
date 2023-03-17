use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, VecDeque};
use std::convert::Into;
use std::ffi::OsStr;
use std::fmt::Write;
use std::fs::{copy, create_dir_all};
use std::num::{NonZeroI32, NonZeroU32, NonZeroUsize};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use dialoguer::theme::ColorfulTheme;
use dialoguer::{History, Input};
use image::Rgba;
use lru::LruCache;
#[cfg(feature = "opencl")]
use once_cell::sync::Lazy;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::config::{load_properties, string_to_colour, ImageProperties, Properties, CONFIG};
use crate::directories::ids::{relative_from_slash, TempWallpaperID, WallpaperID};
use crate::directories::{
    next_original_for_prefix, next_original_for_wildcard_prefix, next_original_in_dir,
    valid_extension,
};
use crate::monitors::set_wallpapers;
#[cfg(feature = "opencl")]
use crate::processing::resample::OPENCL_QUEUE;
use crate::wallpaper::{Wallpaper, OPTIMISTIC_CACHE};
use crate::{closing, make_tdir, monitors};

#[derive(Debug)]
enum Command {
    Vertical(f64),
    Horizontal(f64),
    Top(i32),
    Bottom(i32),
    Left(i32),
    Right(i32),
    Background(Rgba<u8>),
    Denoise(i32),
    Install(String, Option<(NonZeroU32, NonZeroU32)>),
    Update(Option<(NonZeroU32, NonZeroU32)>),
    // Reset to initial state.
    // Equivalent to clear when there is no configuration present in .properties.toml
    Reset,
    // Zero out all properties
    Clear,
    Help,
    Print,
    Exit,
    Invalid,
}

impl From<&str> for Command {
    fn from(s: &str) -> Self {
        let s = s.to_ascii_lowercase();
        let trimmed = s.trim();
        let (left, right) = match trimmed.split_once(' ') {
            Some((a, b)) => (a, b.trim()),
            None => (trimmed, ""),
        };

        let i = right.parse::<i32>().ok();
        let f = right.parse::<f64>().ok();


        match (left, i, f) {
            ("vertical" | "v", _, Some(f)) => Self::Vertical(f),
            ("horizontal" | "h", _, Some(f)) => Self::Horizontal(f),
            ("top" | "t", Some(i), ..) => Self::Top(i),
            ("bottom" | "b", Some(i), ..) => Self::Bottom(i),
            ("left" | "l", Some(i), ..) => Self::Left(i),
            ("right" | "r", Some(i), ..) => Self::Right(i),
            ("background" | "bg", ..) => {
                string_to_colour(right).map_or_else(|| Self::Invalid, Self::Background)
            }
            ("install", ..) => {
                parse_install(right).map_or_else(|_| Self::Invalid, |(a, b)| Self::Install(a, b))
            }
            ("update", ..) => parse_res(right).map_or_else(|_| Self::Invalid, Self::Update),
            ("denoise" | "d", Some(i), ..) => Self::Denoise(i),
            ("reset", ..) => Self::Reset,
            ("clear", ..) => Self::Clear,
            ("help", ..) => Self::Help,
            ("print", ..) => Self::Print,
            ("exit", ..) => Self::Exit,
            _ => Self::Invalid,
        }
    }
}

impl Command {
    const fn process(&self) -> bool {
        match self {
            Self::Vertical(_)
            | Self::Horizontal(_)
            | Self::Top(_)
            | Self::Bottom(_)
            | Self::Left(_)
            | Self::Right(_)
            | Self::Background(_)
            | Self::Denoise(_)
            | Self::Reset
            | Self::Clear => true,
            Self::Install(..)
            | Self::Update(_)
            | Self::Help
            | Self::Print
            | Self::Exit
            | Self::Invalid => false,
        }
    }
}


#[tokio::main(flavor = "current_thread")]
pub async fn run(starting_path: &Path) {
    let tdir = make_tdir();

    let monitors = monitors::list();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    #[cfg(feature = "opencl")]
    let cl_spawn_handle = thread::spawn(|| {
        Lazy::force(&OPENCL_QUEUE);
    });

    if monitors::supports_memory_papers() {
        OPTIMISTIC_CACHE.get_or_init(|| {
            Mutex::new(LruCache::new(NonZeroUsize::new(monitors.len() * 3).unwrap()))
        });
    }

    let wid = TempWallpaperID::new(starting_path, ImageProperties::default(), &tdir);


    let initial_props = if let Some(slash_path) = wid.slash_path() {
        let mut properties = load_properties();

        if let Some(mut props) = properties.remove(&slash_path) {
            println!("Loaded configured properties:\n{props}");

            if !props.nested.is_empty() {
                props.nested.clear(); // Doesn't matter for now, but do it anyway
                println!("Found per-monitor settings. They are ignored in interactive mode.");
            }

            wid.props.write().unwrap().clone_from(&props);

            props
        } else {
            ImageProperties::default()
        }
    } else {
        ImageProperties::default()
    };

    println!("Previewing...");

    let wallpaper = Wallpaper::new(&wid, &monitors, &tdir);
    wallpaper.process(false);
    set_wallpapers(&[(&wid, &monitors)], true);

    // It'll be done by now, just join it to be certain.
    #[cfg(feature = "opencl")]
    cl_spawn_handle.join().unwrap();

    // Just checking the status of the closing atomic in a loop is good enough. If the user hits
    // CTRL-C the Input handler will exit immediately.
    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let (comp_sender, comp_receiver) = mpsc::unbounded_channel();

    thread::spawn(move || {
        console(sender, comp_receiver);
    });

    loop {
        let commands = select! {
            cmds = receiver.recv() => {
                if let Some(cmds) = cmds {
                     cmds
                } else {
                     return;
                }
            }
            _ = ticker.tick() => {
                if closing::closed() {
                    return;
                }
                continue;
            }
        };

        let mut props = wid.props.write().unwrap();
        let mut process = false;

        for command in commands {
            process = process || command.process();

            match command {
                Command::Vertical(v) => props.vertical = if v == 0.0 { None } else { Some(v) },
                Command::Horizontal(h) => props.horizontal = if h == 0.0 { None } else { Some(h) },
                Command::Top(t) => props.top = NonZeroI32::new(t).map(Into::into),
                Command::Bottom(b) => props.bottom = NonZeroI32::new(b).map(Into::into),
                Command::Left(l) => props.left = NonZeroI32::new(l).map(Into::into),
                Command::Right(r) => props.right = NonZeroI32::new(r).map(Into::into),
                Command::Background(bg) => {
                    props.background = if bg == [0, 0, 0, 0xff].into() { None } else { Some(bg) }
                }
                Command::Denoise(d) => props.denoise = if d != 1 { Some(d) } else { None },
                Command::Install(rel, res) => {
                    let rel = relative_from_slash(rel);
                    if let Some(new_path) = install(rel, &wid.original_abs_path()) {
                        wid.set_original_path(new_path);
                        if !props.is_empty() {
                            drop(props);
                            update_properties(&wid, res);
                            props = wid.props.write().unwrap();
                        }
                    }
                }
                Command::Update(res) => {
                    drop(props);
                    update_properties(&wid, res);
                    props = wid.props.write().unwrap();
                }
                Command::Reset => {
                    props.clone_from(&initial_props);
                }
                Command::Clear => {
                    *props = ImageProperties::default();
                }
                Command::Help => {
                    // TODO -- help
                }
                Command::Print => {
                    println!("{props}");
                }
                Command::Exit => return,
                Command::Invalid => {
                    println!("Invalid command");
                    // TODO -- help
                }
            }
        }
        drop(props);

        if process {
            // let start = Instant::now();
            wallpaper.process(false);
            // println!("process {:?}", start.elapsed());
            // let set = Instant::now();
            set_wallpapers(&[(&wid, &monitors)], true);
            // println!("set {:?} / {:?}", set.elapsed(), start.elapsed());
        }

        comp_sender.send(()).unwrap();
    }
}

fn install(rel: PathBuf, original: &Path) -> Option<PathBuf> {
    if original.starts_with(&CONFIG.originals_directory) {
        // TODO -- could this be used for renaming?
        // Need to handle moving any existing properties, or leave them for manual cleanup.
        println!("Can't install file that is already inside the originals directory");
        return None;
    }

    if rel.is_absolute() {
        println!("Install path must be relative.");
        return None;
    }

    let mut dest = normalize_path(&CONFIG.originals_directory.join(rel));

    if !dest.starts_with(&CONFIG.originals_directory) {
        println!("Cannot install file outside of the originals directory.");
        return None;
    }

    match (
        dest.extension().map(OsStr::to_ascii_lowercase),
        original.extension().map(OsStr::to_ascii_lowercase),
    ) {
        (Some(a), Some(b)) if a == "*" && valid_extension(&b) => {
            dest.set_extension(b);
        }
        (_, Some(e)) | (Some(e), None) if !valid_extension(&e) => {
            println!("Extension {e:?} is not valid for an original wallpaper, unable to install.");
            return None;
        }
        (Some(a), Some(b)) if a == b => {}

        // Empty file name, like when the user types "existing_or_new_dir/"
        // If a new directory is provided, use the empty string as the prefix.
        (None, Some(e)) if dest.file_name().map_or(true, |n| n == "*") => {
            let dir = dest.parent()?;
            let Some((mut p, n, digits)) = next_original_in_dir(dir) else {
                println!("Could not determine name for next file in {dir:?}");
                return None;
            };

            p.push(format!("{n:0>digits$}."));
            p.push(e);
            dest = dir.join(p);
        }

        // No file name, but matches a directory, like when the user types "existing_dir"
        (None, Some(e)) if dest.is_dir() => {
            // match next_original_in_dir(&dest) {
            let Some((mut p, n, digits)) = next_original_in_dir(&dest) else {
                println!("Could not determine name for next file in {dest:?}");
                return None;
            };

            p.push(format!("{n:0>digits$}."));
            p.push(e);
            dest = dest.join(p);
        }

        // Could skip conversion, unlikely to matter in practice
        (None, Some(e)) if dest.as_os_str().to_string_lossy().ends_with('*') => {
            let dir = dest.parent()?;
            let prefix = dest.file_name()?;
            let mut prefix = prefix.to_string_lossy()[0..prefix.len() - 1].to_string();

            let Some((n, digits)) = next_original_for_wildcard_prefix(dir, &prefix) else {
                    println!(
                        "Could not determine name for next file in {dir:?} starting with \
                         {prefix:?}"
                    );
                    return None;
            };

            write!(prefix, "{n:0>digits$}.").unwrap();
            prefix.push_str(&e.to_string_lossy());
            dest = dir.join(prefix);
        }

        // Partial file name treated as a prefix provided it can identify a single sequence of
        // existing files.
        //
        // Matches for "prefix" when there are files named "prefix001.png" or
        // ("prefix_and_some_more_01.png" and "prefix_and_some_more_02.png") but
        // not if there are both "prefix_and_some_more_01.png" and
        // "prefix_and_something_else_01.png".
        //
        // There must be an existing file, currently doesn't match directories.
        (None, Some(e)) => {
            let dir = dest.parent()?;
            let prefix = dest.file_name()?.to_string_lossy();

            let Some((mut p, n, digits)) = next_original_for_prefix(dir, &prefix) else {
                    println!(
                        "Could not determine name for next file in {dir:?} starting with \
                         {prefix:?}"
                    );
                    return None;
            };

            p.push(format!("{n:0>digits$}."));
            p.push(e);
            dest = dir.join(p);
        }

        (_, Some(b)) => {
            println!("New extension doesn't match old extension, should be {b:?}");
            return None;
        }
        (None, _) => {
            // TODO -- auto-number if wildcard
            println!("Can't install file without an extension");
            return None;
        }
        // Existing file doesn't have an extension.
        (Some(_), None) => {}
    }

    if !dest.starts_with(&CONFIG.originals_directory) {
        // This should never happen, but check to be certain.
        println!(
            "Cannot install file outside of the originals directory. This should never happen."
        );
        return None;
    }

    if dest.exists() {
        println!("File {dest:?} already exists.");
        return None;
    }

    // We already know the originals_directory must exist, and new_path must have a parent
    if let Err(e) = create_dir_all(dest.parent().unwrap()) {
        println!("Error creating directories: {e}");
        return None;
    }

    // Try moving first, fall back to copy, never delete
    if std::fs::rename(original, &dest).is_ok() {
        println!("Installed wallpaper to {dest:?}");
        return Some(dest);
    }

    match std::fs::copy(original, &dest) {
        Ok(_) => {
            println!("Installed wallpaper to {dest:?}, did not delete {original:?}");
            Some(dest)
        }
        Err(e) => {
            println!("Failed to install file: {e}");
            None
        }
    }
}

fn update_properties(wid: &TempWallpaperID, res: Option<(NonZeroU32, NonZeroU32)>) {
    let Some(slash_path) = wid.slash_path() else {
        println!("Tried to update properties for wallpaper outside of originals directory");
        return;
    };

    let new_props = wid.props.read().unwrap();

    let mut properties = load_properties();

    get_or_insert(&mut properties, &slash_path, res).copy_from(&new_props);
    write_properties(&properties);
}

fn get_or_insert<'a>(
    properties: &'a mut Properties,
    slash_path: &Path,
    res: Option<(NonZeroU32, NonZeroU32)>,
) -> &'a mut ImageProperties {
    // We're not doing this so often that a few clones here are a problem.
    let ip = match properties.entry(slash_path.to_path_buf()) {
        Entry::Vacant(v) => v.insert(ImageProperties::default()),
        Entry::Occupied(o) => o.into_mut(),
    };

    if let Some(res) = res {
        let (x, y) = (res.0.to_string(), res.1.to_string());
        let x_m = match ip.nested.entry(x) {
            Entry::Vacant(v) => v.insert(BTreeMap::new()),
            Entry::Occupied(o) => o.into_mut(),
        };
        match x_m.entry(y) {
            Entry::Vacant(v) => v.insert(ImageProperties::default()),
            Entry::Occupied(o) => o.into_mut(),
        }
    } else {
        ip
    }
}

fn write_properties(props: &Properties) {
    let propfile = CONFIG.originals_directory.join(".properties.toml");
    let backup = CONFIG.originals_directory.join(".properties.toml.bak");
    if propfile.exists() {
        if propfile.is_file() {
            if let Err(e) = copy(&propfile, &backup) {
                println!(
                    "Error: Failed to back up existing proprties file: {e}. Updated properties \
                     have not been written."
                );
                return;
            }
            println!("Backed up existing properties to {backup:?}");
        } else {
            println!(
                "Error: properties file already exists but is not a regular file properties have \
                 not been written"
            );
            return;
        }
    }

    let out = toml::to_string(props).unwrap();
    if let Err(e) = std::fs::write(propfile, out) {
        println!("Failed to write properties to file: {e}");
    }
}

fn console(sender: mpsc::UnboundedSender<Vec<Command>>, mut comp: mpsc::UnboundedReceiver<()>) {
    let mut history = ConsoleHistory::default();

    while let Ok(input) = Input::<String>::with_theme(&ColorfulTheme::default())
        .history_with(&mut history)
        .with_prompt("wallpapers")
        .interact_text()
    {
        let commands = input.split(';').map(Command::from).collect();
        if sender.send(commands).is_err() {
            closing::close();
            return;
        }

        if comp.blocking_recv().is_none() {
            closing::close();
            return;
        }
    }

    drop(sender);
}

struct ConsoleHistory {
    max: usize,
    history: VecDeque<String>,
}

impl Default for ConsoleHistory {
    fn default() -> Self {
        Self { max: 128, history: VecDeque::new() }
    }
}

impl<T: ToString> History<T> for ConsoleHistory {
    fn read(&self, pos: usize) -> Option<String> {
        self.history.get(pos).cloned()
    }

    fn write(&mut self, val: &T) {
        if self.history.len() == self.max {
            self.history.pop_back();
        }
        self.history.push_front(val.to_string());
    }
}

// An empty/whitespace only string is Ok(None), but a non-empty unparseable string is Err(())
#[allow(clippy::many_single_char_names)]
fn parse_res(s: &str) -> Result<Option<(NonZeroU32, NonZeroU32)>, ()> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let (w, h) = match trimmed.split_once('x') {
        Some((a, b)) => (a, b.trim()),
        None => return Err(()),
    };

    let (w, h) = (w.parse::<u32>(), h.parse::<u32>());

    let (w, h) = match (w, h) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 => (w, h),
        _ => return Err(()),
    };

    let (mut a, mut b) = (w, h);
    while b != 0 {
        let c = b;
        b = a % b;
        a = c;
    }

    Ok(Some((NonZeroU32::new(w / a).unwrap(), NonZeroU32::new(h / a).unwrap())))
}

fn parse_install(s: &str) -> Result<(String, Option<(NonZeroU32, NonZeroU32)>), ()> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(());
    }

    let (path, res) = match trimmed.split_once(' ') {
        Some((a, b)) => (a, b.trim()),
        None => return Ok((trimmed.to_string(), None)),
    };

    match parse_res(res) {
        Ok(res) => Ok((path.to_string(), res)),
        // Allow spaces in filenames
        Err(_) => Ok((trimmed.to_string(), None)),
    }
}

// Canonicalize fails to handle files/directories that do not exist.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().copied() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}
