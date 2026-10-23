fn main() {
    let compiler = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("--version")
        .output()
        .expect("rustc version");
    assert!(
        String::from_utf8_lossy(&compiler.stdout).starts_with("rustc 1.99.1 "),
        "Rust 1.99.1 is required"
    );
    println!("cargo:rerun-if-changed=native/audio.c");
    println!("cargo:rerun-if-changed=native/audio.h");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    let libraries = ["libavformat", "libavcodec", "libswresample", "libavutil"];
    let mut includes = Vec::new();
    for library in libraries {
        let found = pkg_config::Config::new().statik(true).cargo_metadata(false)
            .probe(library).expect("Build native dependencies with scripts/bootstrap-static-audio-deps.sh and set PKG_CONFIG_PATH");
        includes.extend(found.include_paths);
        for path in &found.link_paths {
            for lib in &found.libs {
                let archive = path.join(format!("lib{lib}.a"));
                if archive.exists() {
                    println!("cargo:rerun-if-changed={}", archive.display());
                }
            }
        }
    }
    cc::Build::new()
        .file("native/audio.c")
        .includes(includes)
        .flag_if_supported("-std=c11")
        .compile("smartaudio_native");
    for library in libraries {
        pkg_config::Config::new()
            .statik(true)
            .probe(library)
            .unwrap();
    }
}
