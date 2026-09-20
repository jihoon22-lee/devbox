// Tauri copies shared notices into one Cargo profile staging directory. Serialize
// that build-script section; independent Rust compilation can stay parallel.
fn lock_bundle_staging() -> std::fs::File {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let profile = out.ancestors().nth(3).expect("Cargo profile directory");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(profile.join(".devbox-tauri-build.lock"))
        .expect("open private bundle staging lock");
    std::fs::File::lock(&lock).expect("lock private bundle staging");
    lock
}
