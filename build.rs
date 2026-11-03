//! rust-i18n bakes `locales/` into the binary at compile time and asks cargo to
//! watch none of it, so editing a translation is silently ignored until some
//! other file forces a rebuild. Watch the catalog here instead.

fn main() {
    println!("cargo:rerun-if-changed=locales");
    if let Ok(entries) = std::fs::read_dir("locales") {
        for entry in entries.flatten() {
            println!("cargo:rerun-if-changed={}", entry.path().display());
        }
    }
}
