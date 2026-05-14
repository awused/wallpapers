use std::error::Error;
use std::pin::pin;
use std::sync::LazyLock;

use futures::StreamExt;
use libc::SIGUSR1;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook_tokio::Signals;
use tokio::select;

use crate::config::{PROPERTIES, load_properties};
use crate::monitors::{self, Connection};
use crate::random;
use crate::wallpaper::clear_caches;

pub fn run() {
    if let Err(e) = tokio_run() {
        println!("Daemon exited with error {e}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn tokio_run() -> Result<(), Box<dyn Error>> {
    let mut signals = Signals::new(TERM_SIGNALS)?;
    signals.handle().add_signal(SIGUSR1)?;

    let mut con = monitors::init();

    'outer: loop {
        {
            let mut random = pin!(random(&mut con));

            'inner: loop {
                select! {
                    res = &mut random => {
                        match res {
                            Ok(_) => break 'inner,
                            Err(e) => println!("Got unexpected error {e}"),
                        }
                    },
                    sig = signals.next() => {
                        match sig {
                            Some(SIGUSR1) => {
                                println!("Ignoring SIGUSR1 while setting wallpapers");
                            },
                            Some(sig) => {
                                println!("Got signal {sig}, exiting cleanly");
                                break 'outer;
                            },
                            None => unreachable!(),
                        }
                    },
                }
            }
        }

        cleanup();

        select! {
            sig = signals.next() => {
                match sig {
                    Some(SIGUSR1) => {},
                    Some(sig) => {
                        println!("Got signal {sig}, exiting cleanly");
                        break;
                    },
                    None => unreachable!(),
                }
            },
            _ = con.poll() => {
                return Err("con.poll() returned unexpectedly".into());
            }
        }
    }

    Ok(())
}

// Reset state and drop as much memory as possible
fn cleanup() {
    *PROPERTIES.write().unwrap() = LazyLock::new(load_properties);
    clear_caches();
    unsafe {
        libc::malloc_trim(0);
    }
}
