use std::env;

fn main() {
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap();
    if target_family == "unix" {
        println!("cargo:rustc-link-lib=X11");
        println!("cargo:rustc-link-lib=Xrandr");
    }
}
