use std::env;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_dir = Path::new(&out_dir);
    std::fs::create_dir_all(out_dir).unwrap();

    let mut cc = cc::Build::new();
    cc.file("shim.c")
       .include("pulseheaders")
       .flag("-shared")
       .flag("-fPIC")
       .compile("pulse_viz");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=dylib=pulse_viz");
    println!("cargo:rustc-link-arg=-L/usr/lib64");
    println!("cargo:rustc-link-arg=-L{}", out_dir.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib64");
    // Link libpulse via full path to avoid the missing libpulse.so symlink.
    println!("cargo:rustc-link-arg=/usr/lib64/libpulse.so.0");
    println!("cargo:rustc-link-arg=-lpthread");
    println!("cargo:rustc-link-arg=-lm");
}
