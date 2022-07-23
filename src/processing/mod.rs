use std::any::Any;
use std::thread;

use once_cell::sync::Lazy;
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::closing;
use crate::config::CONFIG;

pub mod resample;

// Pre- and post- upscaling work shares the same CPU-bound pool
pub static WORKER: Lazy<ThreadPool> = Lazy::new(|| {
    ThreadPoolBuilder::new()
        .thread_name(|u| format!("worker-{}", u))
        .panic_handler(handle_panic)
        .build()
        .expect("Error creating worker threadpool")
});

pub static UPSCALING: Lazy<ThreadPool> = Lazy::new(|| {
    ThreadPoolBuilder::new()
        .thread_name(|u| format!("upscaling-{}", u))
        .panic_handler(handle_panic)
        .num_threads(CONFIG.upscaling_jobs)
        .build()
        .expect("Error creating upscaling threadpool")
});


fn handle_panic(_e: Box<dyn Any + Send>) {
    println!("Unexpected panic in thread {}", thread::current().name().unwrap_or("unnamed"));
    closing::close();
}
