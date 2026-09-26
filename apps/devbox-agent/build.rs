include!("../../crates/product-shell-tauri/build_support.rs");
fn main() {
    let _staging = lock_bundle_staging();
    tauri_build::build();
}
