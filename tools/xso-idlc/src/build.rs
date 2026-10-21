// build.rs
fn main() {
    let src = std::fs::read_dir("src").unwrap();
    for entry in src.flatten() {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if content.contains("generate(&module)") {
                panic!("found `generate(&module)` in {}; use `generate(&ir)`",
                       entry.path().display());
            }
        }
    }
}