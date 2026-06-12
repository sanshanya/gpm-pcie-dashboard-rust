use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=NVML_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=NVML_LIB_DIR");

    if let Ok(lib_dir) = env::var("NVML_LIB_DIR") {
        println!("cargo:rustc-link-search=native={lib_dir}");
    }
    println!("cargo:rustc-link-lib=dylib=nvidia-ml");

    let include_dir = env::var("NVML_INCLUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/usr/local/cuda/include"));
    let header = include_dir.join("nvml.h");

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .clang_arg(format!("-I{}", include_dir.display()))
        .allowlist_function("nvml.*")
        .allowlist_type("nvml.*")
        .allowlist_var("NVML_GPM_.*")
        .layout_tests(false)
        .generate()
        .expect("failed to generate NVML bindings; set NVML_INCLUDE_DIR to the directory containing nvml.h");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("nvml.rs"))
        .expect("failed to write generated bindings");
}
