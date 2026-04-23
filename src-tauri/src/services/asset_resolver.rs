use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::models::WallpaperRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetResolver {
    managed_root: PathBuf,
    source_root: PathBuf,
    extracted_root: PathBuf,
    decoded_root: PathBuf,
}

impl AssetResolver {
    pub fn for_managed_root(path: impl AsRef<Path>) -> Self {
        let managed_root = path.as_ref().to_path_buf();
        Self {
            source_root: managed_root.join("source"),
            extracted_root: managed_root.join("extracted"),
            decoded_root: managed_root.join("decoded"),
            managed_root,
        }
    }

    pub fn for_record(record: &WallpaperRecord) -> Self {
        Self::for_managed_root(&record.managed_path)
    }

    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    pub fn extracted_root(&self) -> &Path {
        &self.extracted_root
    }

    pub fn decoded_root(&self) -> &Path {
        &self.decoded_root
    }

    pub fn scene_json_path(&self) -> Option<PathBuf> {
        self.first_existing(&[
            self.extracted_root.join("scene.json"),
            self.source_root.join("scene.json"),
        ])
    }

    pub fn scene_pkg_path(&self) -> Option<PathBuf> {
        self.first_existing(&[
            self.source_root.join("scene.pkg"),
            self.managed_root.join("scene.pkg"),
        ])
    }

    pub fn resolve_preview_path(&self, existing: Option<&str>) -> Option<String> {
        existing
            .and_then(|value| self.resolve_existing_path(value))
            .or_else(|| {
                self.find_first_with_extensions(
                    self.source_root(),
                    &["gif", "png", "jpg", "jpeg", "webp"],
                )
            })
            .map(|path| path.display().to_string())
    }

    pub fn resolve_entry_path(
        &self,
        existing: Option<&str>,
        wallpaper_type: &str,
    ) -> Option<String> {
        existing
            .and_then(|value| self.resolve_existing_path(value))
            .or_else(|| match wallpaper_type {
                "scene" => self.scene_json_path(),
                "video" => self
                    .find_first_with_extensions(self.source_root(), &["mp4", "webm", "mov", "mkv"]),
                "web" => self.find_first_with_extensions(self.source_root(), &["html", "htm"]),
                _ => None,
            })
            .map(|path| path.display().to_string())
    }

    pub fn resolve_existing_path(&self, value: impl AsRef<Path>) -> Option<PathBuf> {
        let value = value.as_ref();
        let mut candidates = Vec::new();
        if value.is_absolute() {
            candidates.push(value.to_path_buf());
        } else {
            candidates.push(self.source_root.join(value));
            candidates.push(self.extracted_root.join(value));
            candidates.push(self.decoded_root.join(value));
            candidates.push(self.managed_root.join(value));
        }
        self.first_existing(&candidates)
    }

    fn first_existing(&self, candidates: &[PathBuf]) -> Option<PathBuf> {
        candidates.iter().find(|path| path.exists()).cloned()
    }

    fn find_first_with_extensions(&self, root: &Path, extensions: &[&str]) -> Option<PathBuf> {
        if !root.exists() {
            return None;
        }

        let expected = extensions
            .iter()
            .map(|value| value.to_ascii_lowercase())
            .collect::<Vec<_>>();

        let mut stack = vec![root.to_path_buf()];
        while let Some(path) = stack.pop() {
            let Ok(entries) = fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    stack.push(entry_path);
                    continue;
                }
                let Some(extension) = entry_path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_ascii_lowercase())
                else {
                    continue;
                };
                if expected.iter().any(|candidate| candidate == &extension) {
                    return Some(entry_path);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::AssetResolver;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("wallpaper-resolver-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("failed to create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn resolves_scene_paths_with_pkg_extracted_source_fallbacks() {
        let temp = TempDirGuard::new();
        let managed = temp.path.join("managed");
        let source = managed.join("source");
        let extracted = managed.join("extracted");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&extracted).unwrap();
        fs::write(source.join("scene.pkg"), b"pkg").unwrap();
        fs::write(extracted.join("scene.json"), "{}").unwrap();

        let resolver = AssetResolver::for_managed_root(&managed);
        assert_eq!(resolver.scene_pkg_path(), Some(source.join("scene.pkg")));
        assert_eq!(
            resolver.scene_json_path(),
            Some(extracted.join("scene.json"))
        );
    }

    #[test]
    fn resolves_relative_paths_from_primary_roots_in_order() {
        let temp = TempDirGuard::new();
        let managed = temp.path.join("managed");
        let source = managed.join("source");
        let extracted = managed.join("extracted");
        let decoded = managed.join("decoded");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&extracted).unwrap();
        fs::create_dir_all(&decoded).unwrap();

        fs::write(source.join("preview.png"), b"source").unwrap();
        fs::write(extracted.join("preview-alt.png"), b"extracted").unwrap();
        fs::write(decoded.join("decoded.png"), b"decoded").unwrap();

        let resolver = AssetResolver::for_managed_root(&managed);
        assert_eq!(
            resolver.resolve_existing_path("preview.png"),
            Some(source.join("preview.png"))
        );
        assert_eq!(
            resolver.resolve_existing_path("preview-alt.png"),
            Some(extracted.join("preview-alt.png"))
        );
        assert_eq!(
            resolver.resolve_existing_path("decoded.png"),
            Some(decoded.join("decoded.png"))
        );
    }
}
