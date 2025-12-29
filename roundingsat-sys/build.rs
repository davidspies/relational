use std::path::PathBuf;
use std::process::Command;

fn main() {
    let roundingsat_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("vendor/roundingsat");

    let build_dir = roundingsat_dir.join("build_lib");
    let lib_path = build_dir.join("libroundingsat.a");

    // Build with cmake if library doesn't exist
    if !lib_path.exists() {
        std::fs::create_dir_all(&build_dir).expect("Failed to create build directory");

        // Use RelWithDebInfo for -O2 -g, but undefine NDEBUG to keep asserts enabled
        let status = Command::new("cmake")
            .args(["-DCMAKE_BUILD_TYPE=RelWithDebInfo", "-DCMAKE_CXX_FLAGS_RELWITHDEBINFO=-O2 -g", ".."])
            .current_dir(&build_dir)
            .status()
            .expect("Failed to run cmake");
        assert!(status.success(), "cmake configuration failed");

        let status = Command::new("make")
            .args(["-j4", "roundingsat_lib"])
            .current_dir(&build_dir)
            .status()
            .expect("Failed to run make");
        assert!(status.success(), "make failed");
    }

    // Link the pre-built library
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-lib=static=roundingsat");

    // Link C++ standard library
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }

    // Rerun if source files change
    println!(
        "cargo:rerun-if-changed={}",
        roundingsat_dir.join("src").display()
    );
}
