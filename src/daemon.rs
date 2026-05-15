use std::pin::pin;
use std::sync::LazyLock;
use std::sync::atomic::Ordering;
use std::time::Duration;

use color_eyre::Result;
use futures::StreamExt;
use libc::{SIGUSR1, SIGUSR2};
use signal_hook::consts::TERM_SIGNALS;
use signal_hook_tokio::Signals;
use tokio::select;
use tokio::time::sleep;

use crate::config::{PROPERTIES, load_properties};
use crate::monitors::{self};
use crate::processing::SMALL_POOLS;
use crate::random;
use crate::wallpaper::clear_caches;

pub async fn run() {
    // Prioritize a small footprint over completing things quickly
    SMALL_POOLS.store(true, Ordering::Relaxed);

    if let Err(e) = tokio_run().await {
        println!("Daemon exited with error {e}");
    }
}

async fn tokio_run() -> Result<()> {
    let mut signals = Signals::new(TERM_SIGNALS)?;
    signals.handle().add_signal(SIGUSR1)?;
    signals.handle().add_signal(SIGUSR2)?;

    let mut con = monitors::init();
    let mut monitors = con.list_monitors().await?;

    'outer: loop {
        {
            let mut random = pin!(random(&mut con, monitors));

            'inner: loop {
                select! {
                    res = &mut random => {
                        if let Err(e) = res {
                            // Errors from random() can be ignored as temporary
                            // If there's a problem with the connection, it should fail
                            // in list_monitors() which will be fatal.
                            println!("Got unexpected error: {e}");
                        }
                        break 'inner;
                    },
                    sig = signals.next() => {
                        match sig {
                            Some(SIGUSR1) => {
                                println!("Ignoring SIGUSR1 while setting wallpapers");
                            },
                            Some(SIGUSR2) => {
                                println!("Got SIGUSR2, exiting in one second");
                                sleep(Duration::from_secs(1)).await;
                                break 'outer;
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
                    Some(SIGUSR2) => {
                        println!("Got SIGUSR2, exiting in one second");
                        sleep(Duration::from_secs(1)).await;
                        break 'outer;
                    },
                    Some(sig) => {
                        println!("Got signal {sig}, exiting cleanly");
                        break;
                    },
                    None => unreachable!(),
                }
            },
            res = con.poll() => {
                monitors = res?;
                println!("Got updates to {} monitors", monitors.len());
                continue
            }
        }

        monitors = con.list_monitors().await?;
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
