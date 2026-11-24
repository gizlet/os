//! With `gizlet-os`, raylib-sys links a raylib that is built elsewhere. The card's
//! raylib is a static DRM build, so its own dependencies are linked here.

fn main() {
    println!("cargo:rerun-if-env-changed=GIZLET_RAYLIB_LIB");
    if std::env::var_os("CARGO_FEATURE_GIZLET_OS").is_none() {
        return;
    }
    if let Some(dir) = std::env::var_os("GIZLET_RAYLIB_LIB") {
        println!("cargo:rustc-link-search=native={}", dir.to_string_lossy());
    }
    // Link arguments come after every library on the command line, so these
    // resolve what the static libraylib.a leaves undefined.
    for lib in ["drm", "m", "pthread", "dl"] {
        println!("cargo:rustc-link-arg-bins=-l{lib}");
    }
}
