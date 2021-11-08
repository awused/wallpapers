use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsStr;
use std::fs::{copy, create_dir_all};
use std::num::{NonZeroI32, NonZeroU32};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::Duration;

use dialoguer::theme::ColorfulTheme;
use dialoguer::{History, Input};
use image::Rgba;
use once_cell::unsync::Lazy;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::config::{load_properties, string_to_colour, ImageProperties, Properties, CONFIG};
use crate::directories::ids::{TempWallpaperID, WallpaperID, TEMP_PROPS};
use crate::monitors::set_wallpapers;
use crate::wallpaper::Wallpaper;
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
    // Reset, -- TODO
    Install(String, Option<(NonZeroU32, NonZeroU32)>),
    Update(Option<(NonZeroU32, NonZeroU32)>),
    Help,
    Print,
    Exit,
    Invalid,
}

impl From<String> for Command {
    fn from(s: String) -> Self {
        let s = s.to_ascii_lowercase();
        let trimmed = s.trim();
        let (left, right) = match s.split_once(" ") {
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
            ("help", ..) => Self::Help,
            ("print", ..) => Self::Print,
            ("exit", ..) => Self::Exit,
            _ => Self::Invalid,
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

    println!("Previewing...");
    let wid = TempWallpaperID::new(starting_path, &tdir);
    let wallpaper = Wallpaper::new(&wid, &monitors, &tdir);
    wallpaper.process(false);
    set_wallpapers(&[(&wid, &monitors)], true);

    // Just checking the status of the closing atomic in a loop is good enough. If the user hits
    // CTRL-C the Input handler will exit immediately.
    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let (comp_sender, comp_receiver) = mpsc::unbounded_channel();

    let mut properties: Lazy<Properties> = Lazy::new(load_properties);

    thread::spawn(move || {
        console(sender, comp_receiver);
    });

    loop {
        let command;

        select! {
            cmd = receiver.recv() => {
                if let Some(cmd) = cmd {
                     command = cmd;
                } else {
                     return;
                };
            }
            _ = ticker.tick() => {
                if closing::closed() {
                    return;
                }
                continue;
            }
        }

        let mut props = TEMP_PROPS.write().unwrap();
        let mut process = true;
        match command {
            Command::Vertical(v) => props.vertical = if v == 0.0 { None } else { Some(v) },
            Command::Horizontal(h) => props.horizontal = if h == 0.0 { None } else { Some(h) },
            Command::Top(t) => props.top = NonZeroI32::new(t).map(|x| x.into()),
            Command::Bottom(b) => props.bottom = NonZeroI32::new(b).map(|x| x.into()),
            Command::Left(l) => props.left = NonZeroI32::new(l).map(|x| x.into()),
            Command::Right(r) => props.right = NonZeroI32::new(r).map(|x| x.into()),
            Command::Background(bg) => {
                props.background = if bg == [0, 0, 0, 0xff].into() {
                    None
                } else {
                    Some(bg)
                }
            }
            Command::Denoise(d) => props.denoise = if d != 1 { Some(d) } else { None },
            Command::Install(rel, res) => {
                if let Some(new_path) = install(rel, &wid.original_abs_path()) {
                    wid.set_original_path(new_path);
                    drop(props);
                    update_properties(&wid, res, &mut properties);
                    props = TEMP_PROPS.write().unwrap();
                }
                // If file exists or is already in the originals directory, stop
                // Check that extensions match
                process = false;
            }
            Command::Update(res) => {
                drop(props);
                update_properties(&wid, res, &mut properties);
                props = TEMP_PROPS.write().unwrap();
                process = false;
                // If file isn't part of originals directory, stop
            }
            Command::Help => {
                // TODO -- help
                process = false;
            }
            Command::Print => {
                println!("{}", props);
                process = false;
            }
            Command::Exit => return,
            Command::Invalid => {
                println!("Invalid command");
                // TODO -- help
                process = false;
            }
        }
        drop(props);

        if process {
            wallpaper.process(false);
            set_wallpapers(&[(&wid, &monitors)], true);
        }

        comp_sender.send(()).unwrap();
    }
}

fn install(rel: String, original: &Path) -> Option<PathBuf> {
    if original.starts_with(&CONFIG.originals_directory) {
        // TODO -- could this be used for renaming?
        println!("Can't install file that is already inside the originals directory");
        return None;
    }

    let rel: PathBuf = rel.into();
    if rel.is_absolute() {
        println!("Install path must be relative.");
        return None;
    }


    match (
        rel.extension().map(OsStr::to_ascii_lowercase),
        original.extension().map(OsStr::to_ascii_lowercase),
    ) {
        (Some(a), Some(b)) if a == b => (),
        (_, Some(b)) => {
            println!(
                "New extension doesn't match old extension, should be {:?}",
                b
            );
            return None;
        }
        _ => (),
    }

    let new_path = normalize_path(&CONFIG.originals_directory.join(rel));

    if !new_path.starts_with(&CONFIG.originals_directory) {
        println!("Cannot install file outside of the originals directory.");
        return None;
    }

    if new_path.exists() {
        println!("File {:?} already exists.", new_path);
        return None;
    }

    // We already know the originals_directory must exist, and new_path must have a parent
    if let Err(e) = create_dir_all(new_path.parent().unwrap()) {
        println!("Error creating directories: {}", e);
        return None;
    }

    // Try moving first, fall back to copy, never delete
    if std::fs::rename(&original, &new_path).is_ok() {
        println!("Installed wallpaper to {:?}", new_path);
        return Some(new_path);
    }

    match std::fs::copy(original, &new_path) {
        Ok(_) => {
            println!(
                "Installed wallpaper to {:?}, did not delete {:?}",
                new_path, original
            );
            Some(new_path)
        }
        Err(e) => {
            println!("Failed to install file: {}", e);
            None
        }
    }
}

fn update_properties(
    wid: &TempWallpaperID,
    res: Option<(NonZeroU32, NonZeroU32)>,
    properties: &mut Properties,
) {
    let slash_path = if let Some(p) = wid.slash_path() {
        p
    } else {
        println!("Tried to update properties for wallpaper outside of originals directory");
        return;
    };

    let new_props = TEMP_PROPS.read().unwrap();


    get_or_insert(properties, &slash_path, res).copy_from(&new_props);
    write_properties(properties);
}

fn get_or_insert<'a>(
    properties: &'a mut Properties,
    slash_path: &Path,
    res: Option<(NonZeroU32, NonZeroU32)>,
) -> &'a mut ImageProperties {
    if !properties.contains_key(slash_path) {
        properties.insert(slash_path.to_path_buf(), ImageProperties::default());
    }

    let ip = properties.get_mut(slash_path).unwrap();

    if let Some(res) = res {
        let (x, y) = (res.0.to_string(), res.1.to_string());
        if !ip.nested.contains_key(&x) {
            ip.nested.insert(x.clone(), BTreeMap::new());
        }
        let x_m = ip.nested.get_mut(&x).unwrap();
        if !x_m.contains_key(&y) {
            x_m.insert(y.clone(), ImageProperties::default());
        }
        x_m.get_mut(&y).unwrap()
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
                    "Error: Failed to back up existing proprties file: {}. Updated properties \
                     have not been written.",
                    e
                );
                return;
            }
            println!("Backed up existing properties to {:?}", backup);
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
        println!("Failed to write properties to file: {}", e);
    }
}

fn console(sender: mpsc::UnboundedSender<Command>, mut comp: mpsc::UnboundedReceiver<()>) {
    let mut history = ConsoleHistory::default();

    while let Ok(input) = Input::<String>::with_theme(&ColorfulTheme::default())
        .history_with(&mut history)
        .with_prompt("wallpapers")
        .interact_text()
    {
        if sender.send(Command::from(input)).is_err() {
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
        Self {
            max: 128,
            history: VecDeque::new(),
        }
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

    let (w, h) = match trimmed.split_once("x") {
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

    Ok(Some((
        NonZeroU32::new(w / a).unwrap(),
        NonZeroU32::new(h / a).unwrap(),
    )))
}

fn parse_install(s: &str) -> Result<(String, Option<(NonZeroU32, NonZeroU32)>), ()> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(());
    }

    let (path, res) = match trimmed.split_once(" ") {
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
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
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
