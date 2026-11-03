use std::{env, path::PathBuf, process::Command};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let obj = out.join("cec.o");
    let cc = env::var("CC").unwrap_or_else(|_| "cc".into());
    assert!(Command::new(cc)
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-O2",
            "-c",
            "native/cec.c",
            "-o"
        ])
        .arg(&obj)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ar")
        .arg("crs")
        .arg(out.join("libcec_native.a"))
        .arg(obj)
        .status()
        .unwrap()
        .success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=cec_native");
    println!("cargo:rerun-if-changed=native/cec.c");
}
