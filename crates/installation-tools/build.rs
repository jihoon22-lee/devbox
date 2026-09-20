fn main() {
    if std::env::var_os("CARGO_FEATURE_STANDALONE").is_some() {
        tauri_build::build();
    }
}
