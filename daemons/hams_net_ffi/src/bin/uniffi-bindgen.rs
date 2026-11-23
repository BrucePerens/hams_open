//! `cargo run --bin uniffi-bindgen -- generate --library <cdylib> --language kotlin|swift --out-dir DIR`
fn main() {
    uniffi::uniffi_bindgen_main()
}
