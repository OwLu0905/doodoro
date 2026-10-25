// The AppKit helpers in window-shim.m, compiled into the binary.
fn main() {
    println!("cargo:rerun-if-changed=window-shim.m");
    cc::Build::new().file("window-shim.m").flag("-fobjc-arc").compile("window_shim");
    println!("cargo:rustc-link-lib=framework=AppKit");
}
