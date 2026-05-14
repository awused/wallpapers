use std::any::Any;
use std::num::NonZeroUsize;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, available_parallelism};

use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::closing;
use crate::config::CONFIG;

pub mod resample;

pub static SMALL_POOLS: AtomicBool = AtomicBool::new(false);

// Pre- and post- upscaling work shares the same CPU-bound pool
pub static WORKER: LazyLock<ThreadPool> = LazyLock::new(|| {
    // For long-running daemons, prioritize keeping a small footprint
    let num_threads = if SMALL_POOLS.load(Ordering::Relaxed) {
        1
    } else {
        available_parallelism().map_or(4, NonZeroUsize::get)
    };

    ThreadPoolBuilder::new()
        .thread_name(|u| format!("worker-{u}"))
        .panic_handler(handle_panic)
        .num_threads(num_threads)
        .build()
        .expect("Error creating worker threadpool")
});

pub static UPSCALING: LazyLock<ThreadPool> = LazyLock::new(|| {
    let num_threads = if SMALL_POOLS.load(Ordering::Relaxed) { 1 } else { CONFIG.upscaling_jobs };

    ThreadPoolBuilder::new()
        .thread_name(|u| format!("upscaling-{u}"))
        .panic_handler(handle_panic)
        .num_threads(num_threads)
        .build()
        .expect("Error creating upscaling threadpool")
});


fn handle_panic(_e: Box<dyn Any + Send>) {
    println!("Unexpected panic in thread {}", thread::current().name().unwrap_or("unnamed"));
    closing::close();
}
