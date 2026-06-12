use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=NVML_INCLUDE_DIR");

    let include_dir = find_include_dir().unwrap_or_else(|| {
        panic!(
            "nvml.h not found. Set NVML_INCLUDE_DIR to the directory containing nvml.h. Tried /usr/local/cuda/include, /usr/include/nvidia-ml, and /usr/include."
        )
    });
    let header = include_dir.join("nvml.h");

    println!("cargo:rerun-if-changed={}", header.display());

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .clang_arg(format!("-I{}", include_dir.display()))
        .allowlist_type("nvml.*")
        .allowlist_var("NVML_GPM_.*")
        .allowlist_var("NVML_DEVICE_NAME_BUFFER_SIZE")
        .allowlist_var("NVML_DEVICE_PCI_BUS_ID_BUFFER_SIZE")
        .layout_tests(false)
        .generate()
        .expect("failed to generate NVML bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("nvml.rs"))
        .expect("failed to write generated bindings");
}

fn find_include_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("NVML_INCLUDE_DIR") {
        let path = PathBuf::from(dir);
        if path.join("nvml.h").exists() {
            return Some(path);
        }
    }

    [
        "/usr/local/cuda/include",
        "/usr/include/nvidia-ml",
        "/usr/include",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.join("nvml.h").exists())
}
