#[cfg(feature = "x11")]
use std::env;

fn main() {
    #[cfg(feature = "x11")]
    {
        let target_family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap();
        if target_family == "unix" {
            println!("cargo:rustc-link-lib=X11");
            println!("cargo:rustc-link-lib=Xinerama");
            println!("cargo:rustc-link-lib=Xrandr");
        }
    }
}
