use std::collections::VecDeque;
use std::num::NonZeroI32;
use std::path::Path;
use std::thread;
use std::time::Duration;

use dialoguer::theme::ColorfulTheme;
use dialoguer::{History, Input};
use image::Rgba;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::config::string_to_colour;
use crate::directories::ids::{TempWallpaperID, TEMP_PROPS};
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
            ("denoise" | "d", Some(i), ..) => Self::Denoise(i),
            ("help", ..) => Self::Help,
            ("print", ..) => Self::Print,
            ("exit", ..) => Self::Exit,
            _ => Self::Invalid,
        }
    }
}


#[tokio::main(flavor = "current_thread")]
pub async fn run(path: &Path) {
    let tdir = make_tdir();

    let monitors = monitors::list();
    if monitors.is_empty() {
        println!("No monitors detected");
        return;
    }

    println!("Previewing...");
    let wid = TempWallpaperID::new(path, &tdir);
    let w = Wallpaper::new(&wid, &monitors, &tdir);
    w.process(false);
    set_wallpapers(&[(&wid, &monitors)], true);

    // Just checking the status of the atomic in a loop is good enough. If the user hits CTRL-C the
    // Input handler will exit immediately.

    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let (sender, mut receiver) = mpsc::unbounded_channel();

    let (comp_sender, comp_receiver) = mpsc::unbounded_channel();

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
            Command::Denoise(d) => props.denoise = NonZeroI32::new(d).map(|x| x.into()),
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
            w.process(false);
            set_wallpapers(&[(&wid, &monitors)], true);
        }

        comp_sender.send(()).unwrap();
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
