fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    prepare_macos_sidecar_names();
    tauri_build::build()
}

fn prepare_macos_sidecar_names() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"),
    );
    let bin_dir = manifest_dir.join("../../../local_llm/bin");
    let source = bin_dir.join("llama-server");
    if !source.is_file() {
        return;
    }

    println!("cargo:rerun-if-changed={}", source.display());

    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "universal-apple-darwin",
    ] {
        let link = bin_dir.join(format!("llama-server-{target}"));
        if link.exists() {
            let meta = std::fs::symlink_metadata(&link)
                .unwrap_or_else(|e| panic!("cannot stat {}: {e}", link.display()));
            if !meta.is_symlink() {
                panic!("{} exists but is not a symlink", link.display());
            }
            let target_path = std::fs::read_link(&link)
                .unwrap_or_else(|e| panic!("cannot read symlink {}: {e}", link.display()));
            if target_path != std::path::Path::new("llama-server") {
                panic!(
                    "symlink {} points to {} instead of llama-server",
                    link.display(),
                    target_path.display()
                );
            }
            continue;
        }
        std::os::unix::fs::symlink("llama-server", &link)
            .unwrap_or_else(|error| panic!("could not prepare {}: {error}", link.display()));
    }
}
