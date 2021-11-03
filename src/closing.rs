use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use once_cell::sync::Lazy;
#[cfg(unix)]
use signal_hook::consts::SIGHUP;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::flag;

static CLOSED: Lazy<Arc<AtomicBool>> = Lazy::new(|| Arc::new(AtomicBool::new(false)));

pub fn closed() -> bool {
    CLOSED.load(Ordering::Relaxed)
}

pub fn close() {
    CLOSED.store(true, Ordering::Relaxed)
}


pub fn init() {
    for sig in TERM_SIGNALS {
        flag::register_conditional_default(*sig, CLOSED.clone()).unwrap();

        flag::register(*sig, CLOSED.clone()).unwrap();
    }

    #[cfg(unix)]
    {
        flag::register_conditional_default(SIGHUP, CLOSED.clone()).unwrap();

        flag::register(SIGHUP, CLOSED.clone()).unwrap();
    }
}
