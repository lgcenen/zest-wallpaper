use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    tauri_build::build();

    #[cfg(target_os = "macos")]
    emit_swift_runtime_rpaths();
}

#[cfg(target_os = "macos")]
fn emit_swift_runtime_rpaths() {
    for path in candidate_swift_runtime_dirs() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
    }
}

#[cfg(target_os = "macos")]
fn candidate_swift_runtime_dirs() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("/usr/lib/swift")];

    if let Some(developer_dir) = developer_dir() {
        candidates.extend([
            developer_dir.join("usr/lib/swift/macosx"),
            developer_dir.join("usr/lib/swift-5.5/macosx"),
            developer_dir.join("Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"),
            developer_dir.join("Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx"),
        ]);
    }

    let mut deduped = Vec::new();
    for path in candidates {
        if path.exists() && !deduped.iter().any(|existing: &PathBuf| existing == &path) {
            deduped.push(path);
        }
    }

    deduped
}

#[cfg(target_os = "macos")]
fn developer_dir() -> Option<PathBuf> {
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    if let Ok(path) = std::env::var("DEVELOPER_DIR") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(Path::new(trimmed).to_path_buf());
        }
    }

    println!("cargo:rerun-if-changed=/usr/bin/xcode-select");
    let output = Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(Path::new(trimmed).to_path_buf())
    }
}
