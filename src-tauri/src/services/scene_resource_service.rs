use std::{
    collections::BTreeSet,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::services::{
    asset_resolver::AssetResolver,
    scene_font_alias_service::mac_family_candidates_for_system_font_reference,
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum SceneResourceRootKind {
    ExtractedContent,
    SourceContent,
    DecodedTextureCache,
    ManagedLibrary,
    ScenePackageArchive,
    EffectPackage,
    ExternalAssets,
    BuiltinAssets,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SceneTextFontReferenceKind {
    PathLike,
    SystemFontAlias,
    FamilyLike,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneResourceRoot {
    pub kind: SceneResourceRootKind,
    pub path: PathBuf,
    pub exists: bool,
    pub searchable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneResourceResolver {
    managed_root: PathBuf,
    source_root: PathBuf,
    extracted_root: PathBuf,
    decoded_root: PathBuf,
    scene_pkg_path: PathBuf,
    external_assets_root: Option<PathBuf>,
    builtin_assets_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTextFontCandidates {
    pub authored_reference: String,
    pub reference_kind: SceneTextFontReferenceKind,
    pub file_candidates: Vec<PathBuf>,
    pub family_candidates: Vec<String>,
    pub cache_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneResourceLookup {
    pub authored_reference: String,
    pub attempted_roots: Vec<SceneResourceRoot>,
    pub attempted_candidates: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_root_kind: Option<SceneResourceRootKind>,
    pub external_assets_available: bool,
    pub builtin_assets_available: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneTextFontLookup {
    #[serde(flatten)]
    pub lookup: SceneResourceLookup,
    pub reference_kind: SceneTextFontReferenceKind,
    pub family_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTextureResourceLookup {
    pub lookup: SceneResourceLookup,
    pub matched_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneShaderSourceKind {
    Missing,
    Metal,
    AuthoredSourceSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneShaderSourceLookup {
    pub lookup: SceneResourceLookup,
    pub matched_paths: Vec<PathBuf>,
    pub metal_source_path: Option<PathBuf>,
    pub kind: SceneShaderSourceKind,
}

impl SceneResourceResolver {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn for_managed_root_with_builtin_root(
        managed_root: impl AsRef<Path>,
        builtin_assets_root: impl AsRef<Path>,
    ) -> Self {
        Self::for_managed_root_with_asset_roots(managed_root, builtin_assets_root, None::<PathBuf>)
    }

    pub fn for_managed_root_with_asset_roots(
        managed_root: impl AsRef<Path>,
        builtin_assets_root: impl AsRef<Path>,
        external_assets_root: Option<PathBuf>,
    ) -> Self {
        let managed_root = managed_root.as_ref().to_path_buf();
        let resolver = AssetResolver::for_managed_root(&managed_root);
        let scene_pkg_path = resolver
            .scene_pkg_path()
            .unwrap_or_else(|| managed_root.join("scene.pkg"));

        Self {
            source_root: resolver.source_root().to_path_buf(),
            extracted_root: resolver.extracted_root().to_path_buf(),
            decoded_root: resolver.decoded_root().to_path_buf(),
            scene_pkg_path,
            external_assets_root,
            builtin_assets_root: builtin_assets_root.as_ref().to_path_buf(),
            managed_root,
        }
    }

    pub fn scene_json_path(&self) -> Option<PathBuf> {
        [
            self.extracted_root.join("scene.json"),
            self.source_root.join("scene.json"),
        ]
        .into_iter()
        .find(|path| path.is_file())
    }

    pub fn resource_roots(&self) -> Vec<SceneResourceRoot> {
        let mut roots = vec![
            SceneResourceRoot {
                kind: SceneResourceRootKind::ExtractedContent,
                exists: self.extracted_root.exists(),
                path: self.extracted_root.clone(),
                searchable: true,
            },
            SceneResourceRoot {
                kind: SceneResourceRootKind::SourceContent,
                exists: self.source_root.exists(),
                path: self.source_root.clone(),
                searchable: true,
            },
            SceneResourceRoot {
                kind: SceneResourceRootKind::DecodedTextureCache,
                exists: self.decoded_root.exists(),
                path: self.decoded_root.clone(),
                searchable: true,
            },
            SceneResourceRoot {
                kind: SceneResourceRootKind::ManagedLibrary,
                exists: self.managed_root.exists(),
                path: self.managed_root.clone(),
                searchable: true,
            },
            SceneResourceRoot {
                kind: SceneResourceRootKind::ScenePackageArchive,
                exists: self.scene_pkg_path.exists(),
                path: self.scene_pkg_path.clone(),
                searchable: false,
            },
        ];

        if let Some(path) = self.external_assets_root.clone() {
            roots.push(SceneResourceRoot {
                kind: SceneResourceRootKind::ExternalAssets,
                exists: path.exists(),
                path,
                searchable: true,
            });
        }

        roots.push(SceneResourceRoot {
            kind: SceneResourceRootKind::BuiltinAssets,
            exists: self.builtin_assets_root.exists(),
            path: self.builtin_assets_root.clone(),
            searchable: true,
        });

        roots
    }

    pub fn resolve_relative_path(&self, value: impl AsRef<Path>) -> Option<PathBuf> {
        let value = value.as_ref();
        if value.is_absolute() {
            return value.is_file().then(|| value.to_path_buf());
        }

        self.search_candidates(value)
            .into_iter()
            .find(|candidate| candidate.is_file())
    }

    pub fn inspect_relative_path(&self, value: impl AsRef<Path>) -> SceneResourceLookup {
        let value = value.as_ref();
        let attempted_candidates = if value.is_absolute() {
            vec![value.to_path_buf()]
        } else {
            self.search_candidates(value)
        };
        let matched_path = attempted_candidates
            .iter()
            .find(|candidate| candidate.is_file())
            .cloned();

        SceneResourceLookup {
            authored_reference: value.display().to_string(),
            attempted_roots: self.resource_roots(),
            attempted_candidates,
            matched_root_kind: matched_path
                .as_deref()
                .and_then(|path| self.root_kind_for_path(path)),
            matched_path,
            external_assets_available: self
                .external_assets_root
                .as_ref()
                .map(|path| path.exists())
                .unwrap_or(false),
            builtin_assets_available: self.builtin_assets_root.exists(),
        }
    }

    #[allow(dead_code)]
    pub fn resolve_relative_path_with_local_root(
        &self,
        value: impl AsRef<Path>,
        local_root_kind: SceneResourceRootKind,
        local_root: impl AsRef<Path>,
    ) -> Option<PathBuf> {
        self.inspect_relative_path_with_local_root(value, local_root_kind, local_root)
            .matched_path
    }

    pub fn inspect_relative_path_with_local_root(
        &self,
        value: impl AsRef<Path>,
        local_root_kind: SceneResourceRootKind,
        local_root: impl AsRef<Path>,
    ) -> SceneResourceLookup {
        let value = value.as_ref();
        let local_roots = self.equivalent_local_roots(local_root_kind, local_root.as_ref());
        let attempted_candidates = if value.is_absolute() {
            vec![value.to_path_buf()]
        } else {
            self.search_candidates_with_local_roots(value, &local_roots)
        };
        let matched_path = attempted_candidates
            .iter()
            .find(|candidate| candidate.is_file())
            .cloned();

        SceneResourceLookup {
            authored_reference: value.display().to_string(),
            attempted_roots: local_roots
                .iter()
                .cloned()
                .chain(self.resource_roots())
                .collect(),
            attempted_candidates,
            matched_root_kind: matched_path
                .as_deref()
                .and_then(|path| self.root_kind_for_path_with_local_roots(path, &local_roots)),
            matched_path,
            external_assets_available: self
                .external_assets_root
                .as_ref()
                .map(|path| path.exists())
                .unwrap_or(false),
            builtin_assets_available: self.builtin_assets_root.exists(),
        }
    }

    pub fn resolve_text_font(&self, font_reference: &str) -> SceneTextFontCandidates {
        let authored_reference = font_reference.trim().to_string();
        let reference_kind = scene_text_font_reference_kind(&authored_reference)
            .unwrap_or(SceneTextFontReferenceKind::FamilyLike);
        let family_candidates =
            scene_text_font_family_candidates(&authored_reference, reference_kind);
        let file_candidates =
            self.resolve_text_font_file_candidates(&authored_reference, reference_kind);

        SceneTextFontCandidates {
            cache_key: scene_text_font_candidates_cache_key(
                &authored_reference,
                reference_kind,
                &file_candidates,
                &family_candidates,
            ),
            authored_reference,
            reference_kind,
            file_candidates,
            family_candidates,
        }
    }

    pub fn inspect_text_font(&self, font_reference: &str) -> SceneTextFontLookup {
        let authored_reference = font_reference.trim().to_string();
        let reference_kind = scene_text_font_reference_kind(&authored_reference)
            .unwrap_or(SceneTextFontReferenceKind::FamilyLike);
        let family_candidates =
            scene_text_font_family_candidates(&authored_reference, reference_kind);
        let file_candidates =
            self.resolve_text_font_file_candidates(&authored_reference, reference_kind);
        let attempted_candidates =
            self.text_font_candidate_paths(&authored_reference, reference_kind, false);
        let matched_path = file_candidates.first().cloned();

        SceneTextFontLookup {
            lookup: SceneResourceLookup {
                authored_reference,
                attempted_roots: self.resource_roots(),
                attempted_candidates,
                matched_root_kind: matched_path
                    .as_deref()
                    .and_then(|path| self.root_kind_for_path(path)),
                matched_path,
                external_assets_available: self
                    .external_assets_root
                    .as_ref()
                    .map(|path| path.exists())
                    .unwrap_or(false),
                builtin_assets_available: self.builtin_assets_root.exists(),
            },
            reference_kind,
            family_candidates,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn resolve_texture_candidates(
        &self,
        material_path: Option<&str>,
        texture_name: &str,
    ) -> Vec<PathBuf> {
        self.inspect_texture_candidates(material_path, None, texture_name)
            .matched_paths
    }

    pub fn resolve_texture_candidates_with_local_root(
        &self,
        material_path: Option<&str>,
        material_file_path: Option<&Path>,
        texture_name: &str,
        local_root_kind: SceneResourceRootKind,
        local_root: impl AsRef<Path>,
    ) -> Vec<PathBuf> {
        self.inspect_texture_candidates_with_local_root(
            material_path,
            material_file_path,
            texture_name,
            local_root_kind,
            local_root,
        )
        .matched_paths
    }

    pub fn inspect_texture_candidates(
        &self,
        material_path: Option<&str>,
        material_file_path: Option<&Path>,
        texture_name: &str,
    ) -> SceneTextureResourceLookup {
        self.inspect_texture_candidates_inner(material_path, material_file_path, texture_name, &[])
    }

    pub fn inspect_texture_candidates_with_local_root(
        &self,
        material_path: Option<&str>,
        material_file_path: Option<&Path>,
        texture_name: &str,
        local_root_kind: SceneResourceRootKind,
        local_root: impl AsRef<Path>,
    ) -> SceneTextureResourceLookup {
        self.inspect_texture_candidates_inner(
            material_path,
            material_file_path,
            texture_name,
            &self.equivalent_local_roots(local_root_kind, local_root.as_ref()),
        )
    }

    pub fn inspect_shader_source(&self, shader_reference: &str) -> SceneShaderSourceLookup {
        self.inspect_shader_source_inner(shader_reference, &[])
    }

    pub fn inspect_shader_source_with_local_root(
        &self,
        shader_reference: &str,
        local_root_kind: SceneResourceRootKind,
        local_root: impl AsRef<Path>,
    ) -> SceneShaderSourceLookup {
        self.inspect_shader_source_inner(
            shader_reference,
            &self.equivalent_local_roots(local_root_kind, local_root.as_ref()),
        )
    }

    fn inspect_texture_candidates_inner(
        &self,
        material_path: Option<&str>,
        material_file_path: Option<&Path>,
        texture_name: &str,
        local_roots: &[SceneResourceRoot],
    ) -> SceneTextureResourceLookup {
        let texture_path = Path::new(texture_name);
        let has_relative_segments = texture_path.components().count() > 1;
        let material_dirs = self.material_search_dirs(material_path, material_file_path);
        let mut raw_candidates = Vec::new();
        let mut matched_paths = Vec::new();

        let mut push = |candidate: PathBuf| {
            if candidate.is_file() && !matched_paths.contains(&candidate) {
                matched_paths.push(candidate);
            }
        };
        let mut register_raw = |candidate: PathBuf| {
            if !raw_candidates.contains(&candidate) {
                raw_candidates.push(candidate);
            }
        };

        if texture_path.is_absolute() {
            register_raw(texture_path.to_path_buf());
        } else {
            for root in local_roots {
                register_texture_candidates_for_root(
                    &root.path,
                    texture_path,
                    texture_name,
                    has_relative_segments,
                    &mut register_raw,
                );
            }

            for root in self.raw_search_roots() {
                register_texture_candidates_for_root(
                    root,
                    texture_path,
                    texture_name,
                    has_relative_segments,
                    &mut register_raw,
                );
            }
        }

        for material_dir in material_dirs {
            if texture_path.extension().is_some() {
                register_raw(material_dir.join(texture_path));
            } else {
                register_raw(material_dir.join(texture_path).with_extension("tex"));
            }
        }

        for raw_candidate in &raw_candidates {
            if let Some(decoded_candidate) = self.decoded_candidate_for(raw_candidate) {
                push(decoded_candidate.with_extension("png"));
                push(decoded_candidate.with_extension("mp4"));
            }
        }
        for raw_candidate in &raw_candidates {
            push(raw_candidate.clone());
        }

        let matched_path = matched_paths.first().cloned();
        let lookup = SceneResourceLookup {
            authored_reference: texture_name.to_string(),
            attempted_roots: local_roots
                .iter()
                .cloned()
                .chain(self.resource_roots())
                .collect(),
            attempted_candidates: raw_candidates,
            matched_root_kind: matched_path
                .as_deref()
                .and_then(|path| self.root_kind_for_path_with_local_roots(path, local_roots)),
            matched_path,
            external_assets_available: self
                .external_assets_root
                .as_ref()
                .map(|path| path.exists())
                .unwrap_or(false),
            builtin_assets_available: self.builtin_assets_root.exists(),
        };

        SceneTextureResourceLookup {
            lookup,
            matched_paths,
        }
    }

    fn search_candidates(&self, value: &Path) -> Vec<PathBuf> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();

        for root in self.searchable_roots() {
            let candidate = root.join(value);
            if seen.insert(candidate.clone()) {
                ordered.push(candidate);
            }
        }

        ordered
    }

    fn search_candidates_with_local_roots(
        &self,
        value: &Path,
        local_roots: &[SceneResourceRoot],
    ) -> Vec<PathBuf> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();

        for root in local_roots {
            let candidate = root.path.join(value);
            if seen.insert(candidate.clone()) {
                ordered.push(candidate);
            }
        }

        for candidate in self.search_candidates(value) {
            if seen.insert(candidate.clone()) {
                ordered.push(candidate);
            }
        }

        ordered
    }

    fn inspect_shader_source_inner(
        &self,
        shader_reference: &str,
        local_roots: &[SceneResourceRoot],
    ) -> SceneShaderSourceLookup {
        let shader_reference = shader_reference.trim();
        let shader_path = Path::new(shader_reference);
        let mut attempted_candidates = Vec::new();
        let mut seen = BTreeSet::new();
        let mut register = |candidate: PathBuf| {
            if seen.insert(candidate.clone()) {
                attempted_candidates.push(candidate);
            }
        };

        if shader_path.is_absolute() {
            register(shader_path.to_path_buf());
            if shader_path.extension().is_none() {
                for extension in ["metal", "vert", "frag"] {
                    register(shader_path.with_extension(extension));
                }
            }
        } else {
            for root in local_roots {
                register_shader_source_candidates_for_root(&root.path, shader_path, &mut register);
            }
            for root in self.raw_search_roots() {
                register_shader_source_candidates_for_root(root, shader_path, &mut register);
            }
        }

        let matched_paths = attempted_candidates
            .iter()
            .filter(|candidate| candidate.is_file())
            .cloned()
            .collect::<Vec<_>>();
        let metal_source_path = matched_paths
            .iter()
            .find(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.eq_ignore_ascii_case("metal"))
                    .unwrap_or(false)
            })
            .cloned();
        let kind = if metal_source_path.is_some() {
            SceneShaderSourceKind::Metal
        } else if matched_paths.is_empty() {
            SceneShaderSourceKind::Missing
        } else {
            SceneShaderSourceKind::AuthoredSourceSet
        };
        let matched_path = metal_source_path
            .clone()
            .or_else(|| matched_paths.first().cloned());

        SceneShaderSourceLookup {
            lookup: SceneResourceLookup {
                authored_reference: shader_reference.to_string(),
                attempted_roots: local_roots
                    .iter()
                    .cloned()
                    .chain(self.resource_roots())
                    .collect(),
                attempted_candidates,
                matched_root_kind: matched_path
                    .as_deref()
                    .and_then(|path| self.root_kind_for_path_with_local_roots(path, local_roots)),
                matched_path,
                external_assets_available: self
                    .external_assets_root
                    .as_ref()
                    .map(|path| path.exists())
                    .unwrap_or(false),
                builtin_assets_available: self.builtin_assets_root.exists(),
            },
            matched_paths,
            metal_source_path,
            kind,
        }
    }

    fn resolve_text_font_file_candidates(
        &self,
        font_reference: &str,
        reference_kind: SceneTextFontReferenceKind,
    ) -> Vec<PathBuf> {
        self.text_font_candidate_paths(font_reference, reference_kind, true)
    }

    fn text_font_candidate_paths(
        &self,
        font_reference: &str,
        reference_kind: SceneTextFontReferenceKind,
        existing_only: bool,
    ) -> Vec<PathBuf> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();
        let register =
            |candidate: PathBuf, ordered: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>| {
                if (!existing_only || candidate.exists()) && seen.insert(candidate.clone()) {
                    ordered.push(candidate);
                }
            };

        for relative in scene_text_font_search_paths(font_reference, reference_kind) {
            for root in self.text_font_search_roots() {
                register(root.join(&relative), &mut ordered, &mut seen);
            }
        }

        ordered
    }

    fn root_kind_for_path(&self, path: &Path) -> Option<SceneResourceRootKind> {
        self.resource_roots()
            .into_iter()
            .find_map(|root| path.strip_prefix(&root.path).ok().map(|_| root.kind))
    }

    fn root_kind_for_path_with_local_roots(
        &self,
        path: &Path,
        local_roots: &[SceneResourceRoot],
    ) -> Option<SceneResourceRootKind> {
        self.root_kind_for_path(path).or_else(|| {
            local_roots
                .iter()
                .find_map(|root| path.strip_prefix(&root.path).ok().map(|_| root.kind))
        })
    }

    fn raw_search_roots(&self) -> Vec<&Path> {
        let mut roots = vec![
            self.extracted_root.as_path(),
            self.source_root.as_path(),
            self.managed_root.as_path(),
        ];
        if let Some(root) = self.external_assets_root.as_deref() {
            roots.push(root);
        }
        roots.push(self.builtin_assets_root.as_path());
        roots
    }

    fn text_font_search_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();

        for root in self.raw_search_roots() {
            for candidate in [
                root.to_path_buf(),
                root.join("fonts"),
                root.join("assets"),
                root.join("assets").join("fonts"),
            ] {
                if seen.insert(candidate.clone()) {
                    roots.push(candidate);
                }
            }
        }

        roots
    }

    fn searchable_roots(&self) -> Vec<&Path> {
        let mut roots = vec![
            self.extracted_root.as_path(),
            self.source_root.as_path(),
            self.decoded_root.as_path(),
            self.managed_root.as_path(),
        ];
        if let Some(root) = self.external_assets_root.as_deref() {
            roots.push(root);
        }
        roots.push(self.builtin_assets_root.as_path());
        roots
    }

    fn material_search_dirs(
        &self,
        material_path: Option<&str>,
        material_file_path: Option<&Path>,
    ) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let mut seen = BTreeSet::new();

        if let Some(parent) = material_file_path.and_then(Path::parent) {
            let parent = parent.to_path_buf();
            if seen.insert(parent.clone()) {
                dirs.push(parent);
            }
        }

        let Some(material_path) = material_path else {
            return dirs;
        };
        let material_path = Path::new(material_path);

        for root in self.raw_search_roots() {
            if let Some(parent) = root.join(material_path).parent() {
                let parent = parent.to_path_buf();
                if seen.insert(parent.clone()) {
                    dirs.push(parent);
                }
            }
        }

        dirs
    }

    fn equivalent_local_roots(
        &self,
        local_root_kind: SceneResourceRootKind,
        local_root: &Path,
    ) -> Vec<SceneResourceRoot> {
        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        let register =
            |path: PathBuf, roots: &mut Vec<SceneResourceRoot>, seen: &mut BTreeSet<PathBuf>| {
                if seen.insert(path.clone()) {
                    roots.push(SceneResourceRoot {
                        kind: local_root_kind,
                        exists: path.exists(),
                        path,
                        searchable: true,
                    });
                }
            };

        register(local_root.to_path_buf(), &mut roots, &mut seen);

        let anchor_roots = self.local_root_anchor_roots();
        let relative_path = anchor_roots.iter().find_map(|root| {
            local_root
                .strip_prefix(&root.path)
                .ok()
                .map(Path::to_path_buf)
        });
        let Some(relative_path) = relative_path else {
            return roots;
        };

        for root in anchor_roots {
            register(root.path.join(&relative_path), &mut roots, &mut seen);
        }

        roots
    }

    fn local_root_anchor_roots(&self) -> Vec<SceneResourceRoot> {
        let mut roots = vec![
            SceneResourceRoot {
                kind: SceneResourceRootKind::ExtractedContent,
                exists: self.extracted_root.exists(),
                path: self.extracted_root.clone(),
                searchable: true,
            },
            SceneResourceRoot {
                kind: SceneResourceRootKind::SourceContent,
                exists: self.source_root.exists(),
                path: self.source_root.clone(),
                searchable: true,
            },
            SceneResourceRoot {
                kind: SceneResourceRootKind::ManagedLibrary,
                exists: self.managed_root.exists(),
                path: self.managed_root.clone(),
                searchable: true,
            },
        ];

        if let Some(path) = self.external_assets_root.clone() {
            roots.push(SceneResourceRoot {
                kind: SceneResourceRootKind::ExternalAssets,
                exists: path.exists(),
                path,
                searchable: true,
            });
        }

        roots.push(SceneResourceRoot {
            kind: SceneResourceRootKind::BuiltinAssets,
            exists: self.builtin_assets_root.exists(),
            path: self.builtin_assets_root.clone(),
            searchable: true,
        });

        roots
    }

    fn decoded_candidate_for(&self, candidate: &Path) -> Option<PathBuf> {
        for root in self.raw_search_roots() {
            if let Ok(relative) = candidate.strip_prefix(root) {
                return Some(self.decoded_root.join(relative).with_extension(""));
            }
        }
        None
    }
}

fn register_texture_candidates_for_root(
    root: &Path,
    texture_path: &Path,
    texture_name: &str,
    has_relative_segments: bool,
    register_raw: &mut impl FnMut(PathBuf),
) {
    if texture_path.extension().is_some() {
        register_raw(root.join(texture_path));
        if let Some(sidecar) = texture_tex_sidecar_path(texture_path) {
            register_raw(root.join(&sidecar));
        }
        if has_relative_segments {
            register_raw(root.join("materials").join(texture_path));
            if let Some(sidecar) = texture_tex_sidecar_path(texture_path) {
                register_raw(root.join("materials").join(sidecar));
            }
        }
    } else {
        register_raw(root.join(texture_path).with_extension("tex"));
        if has_relative_segments {
            register_raw(
                root.join("materials")
                    .join(texture_path)
                    .with_extension("tex"),
            );
        } else {
            register_raw(root.join(format!("{texture_name}.tex")));
        }
    }
}

fn texture_tex_sidecar_path(texture_path: &Path) -> Option<PathBuf> {
    let file_name = texture_path.file_name()?.to_str()?;
    let lower = file_name.to_ascii_lowercase();
    let sidecar_name = if lower.ends_with(".tex-json") {
        format!("{}.tex", &file_name[..file_name.len() - ".tex-json".len()])
    } else if lower.ends_with(".tex.json") {
        format!("{}.tex", &file_name[..file_name.len() - ".tex.json".len()])
    } else {
        let extension = texture_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())?;
        if !matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "tga" | "bmp"
        ) {
            return None;
        }
        format!(
            "{}.tex",
            texture_path.file_stem().and_then(|stem| stem.to_str())?
        )
    };
    Some(
        texture_path
            .parent()
            .map(|parent| parent.join(&sidecar_name))
            .unwrap_or_else(|| PathBuf::from(sidecar_name)),
    )
}

fn register_shader_source_candidates_for_root(
    root: &Path,
    shader_path: &Path,
    register: &mut impl FnMut(PathBuf),
) {
    register(root.join(shader_path));
    register(root.join("shaders").join(shader_path));

    if shader_path.extension().is_none() {
        for extension in ["metal", "vert", "frag"] {
            register(root.join(shader_path).with_extension(extension));
            register(
                root.join("shaders")
                    .join(shader_path)
                    .with_extension(extension),
            );
        }
    }
}

pub fn font_reference_looks_like_path(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if value.contains('/') || value.contains('\\') {
        return true;
    }

    Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc" | "woff" | "woff2"
            )
        })
        .unwrap_or(false)
}

pub fn scene_text_font_reference_kind(value: &str) -> Option<SceneTextFontReferenceKind> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if font_reference_looks_like_path(value) {
        return Some(SceneTextFontReferenceKind::PathLike);
    }
    if value
        .get(.."systemfont_".len())
        .map(|prefix| prefix.eq_ignore_ascii_case("systemfont_"))
        .unwrap_or(false)
    {
        return Some(SceneTextFontReferenceKind::SystemFontAlias);
    }
    Some(SceneTextFontReferenceKind::FamilyLike)
}

fn scene_text_font_search_paths(
    font_reference: &str,
    reference_kind: SceneTextFontReferenceKind,
) -> Vec<PathBuf> {
    const FONT_EXTENSIONS: [&str; 6] = ["ttf", "otf", "ttc", "otc", "woff", "woff2"];

    let font_reference = font_reference.trim();
    if font_reference.is_empty() {
        return Vec::new();
    }

    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    let register =
        |candidate: PathBuf, ordered: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>| {
            if seen.insert(candidate.clone()) {
                ordered.push(candidate);
            }
        };

    if reference_kind == SceneTextFontReferenceKind::PathLike {
        let reference_path = PathBuf::from(font_reference);
        register(reference_path.clone(), &mut ordered, &mut seen);
        if reference_path.extension().is_none() {
            for extension in FONT_EXTENSIONS {
                register(
                    reference_path.with_extension(extension),
                    &mut ordered,
                    &mut seen,
                );
            }
        }
        return ordered;
    }

    for reference in scene_text_font_file_lookup_references(font_reference, reference_kind) {
        for stem in scene_text_font_file_stem_candidates(&reference) {
            for base in [
                PathBuf::from(&stem),
                PathBuf::from("fonts").join(&stem),
                PathBuf::from("assets").join(&stem),
                PathBuf::from("assets").join("fonts").join(&stem),
            ] {
                for extension in FONT_EXTENSIONS {
                    register(base.with_extension(extension), &mut ordered, &mut seen);
                }
            }
        }
    }

    ordered
}

fn scene_text_font_file_lookup_references(
    font_reference: &str,
    reference_kind: SceneTextFontReferenceKind,
) -> Vec<String> {
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    let register =
        |candidate: String, references: &mut Vec<String>, seen: &mut BTreeSet<String>| {
            let candidate = candidate.trim().to_string();
            if !candidate.is_empty() && seen.insert(candidate.clone()) {
                references.push(candidate);
            }
        };

    register(font_reference.to_string(), &mut references, &mut seen);
    if reference_kind == SceneTextFontReferenceKind::SystemFontAlias {
        for candidate in scene_text_font_family_candidates(font_reference, reference_kind) {
            register(candidate, &mut references, &mut seen);
        }
    }

    references
}

fn scene_text_font_file_stem_candidates(font_reference: &str) -> Vec<String> {
    let trimmed = font_reference.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let normalized = trimmed
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>();
    let mut stems = Vec::new();
    let mut seen = BTreeSet::new();

    for candidate in [
        trimmed.to_string(),
        collapsed.join(" "),
        collapsed.join("-"),
        collapsed.join("_"),
        collapsed.join(""),
    ] {
        let candidate = candidate.trim();
        if !candidate.is_empty() && seen.insert(candidate.to_string()) {
            stems.push(candidate.to_string());
        }
    }

    stems
}

fn scene_text_font_family_candidates(
    font_reference: &str,
    reference_kind: SceneTextFontReferenceKind,
) -> Vec<String> {
    let mut families = Vec::new();
    let mut seen = BTreeSet::new();
    let register = |candidate: String, families: &mut Vec<String>, seen: &mut BTreeSet<String>| {
        let candidate = candidate.trim().to_string();
        if !candidate.is_empty() && seen.insert(candidate.clone()) {
            families.push(candidate);
        }
    };

    for candidate in authored_font_family_candidates(font_reference, reference_kind) {
        register(candidate, &mut families, &mut seen);
    }

    families
}

fn authored_font_family_candidates(
    font_reference: &str,
    reference_kind: SceneTextFontReferenceKind,
) -> Vec<String> {
    let trimmed = font_reference.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let register =
        |candidate: String, candidates: &mut Vec<String>, seen: &mut BTreeSet<String>| {
            let candidate = candidate.trim().to_string();
            if !candidate.is_empty() && seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
        };

    match reference_kind {
        SceneTextFontReferenceKind::SystemFontAlias => {
            register(trimmed.to_string(), &mut candidates, &mut seen);
            for candidate in mac_family_candidates_for_system_font_reference(trimmed) {
                register(candidate, &mut candidates, &mut seen);
            }
        }
        SceneTextFontReferenceKind::PathLike => {
            if let Some(stem) = Path::new(trimmed)
                .file_stem()
                .and_then(|stem| stem.to_str())
            {
                for variant in scene_text_font_file_stem_candidates(stem) {
                    register(variant, &mut candidates, &mut seen);
                }
            }
        }
        SceneTextFontReferenceKind::FamilyLike => {
            register(trimmed.to_string(), &mut candidates, &mut seen);
            for variant in prettified_font_family_variants(trimmed) {
                register(variant, &mut candidates, &mut seen);
            }
        }
    }

    candidates
}

fn prettified_font_family_variants(font_reference: &str) -> Vec<String> {
    let trimmed = font_reference.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let normalized = trimmed
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    let words = normalized
        .split_whitespace()
        .map(title_case_font_word)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    if words.is_empty() {
        return Vec::new();
    }

    let joined = words.join(" ");
    if joined == trimmed {
        Vec::new()
    } else {
        vec![joined]
    }
}

fn title_case_font_word(word: &str) -> String {
    let mut characters = word.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };

    let mut titled = String::new();
    titled.push(first.to_ascii_uppercase());
    titled.push_str(characters.as_str().to_ascii_lowercase().as_str());
    titled
}

fn scene_text_font_candidates_cache_key(
    authored_reference: &str,
    reference_kind: SceneTextFontReferenceKind,
    file_candidates: &[PathBuf],
    family_candidates: &[String],
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    authored_reference.hash(&mut hasher);
    reference_kind.hash(&mut hasher);

    for candidate in file_candidates {
        candidate.hash(&mut hasher);
        if let Ok(metadata) = fs::metadata(candidate) {
            metadata.len().hash(&mut hasher);
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    duration.as_secs().hash(&mut hasher);
                    duration.subsec_nanos().hash(&mut hasher);
                }
            }
        }
    }

    family_candidates.hash(&mut hasher);
    format!("font:{:x}", hasher.finish())
}

pub fn default_builtin_scene_assets_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("scene")
}

fn builtin_scene_assets_root_from_resource_dir(resource_dir: &Path) -> Option<PathBuf> {
    [
        resource_dir.join("scene"),
        resource_dir.join("resources").join("scene"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

pub fn builtin_scene_assets_root_for_app(app: &AppHandle) -> PathBuf {
    app.path()
        .resource_dir()
        .ok()
        .and_then(|path| builtin_scene_assets_root_from_resource_dir(&path))
        .unwrap_or_else(default_builtin_scene_assets_root)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        default_builtin_scene_assets_root, font_reference_looks_like_path,
        scene_text_font_reference_kind, SceneResourceResolver, SceneResourceRootKind,
        SceneTextFontReferenceKind,
    };

    #[test]
    fn resource_roots_keep_phase_07_lookup_order() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(managed_root.join("decoded")).expect("decoded dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(managed_root.join("scene.pkg"), b"pkg").expect("scene pkg");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let roots = resolver.resource_roots();

        assert_eq!(
            roots.iter().map(|root| root.kind).collect::<Vec<_>>(),
            vec![
                SceneResourceRootKind::ExtractedContent,
                SceneResourceRootKind::SourceContent,
                SceneResourceRootKind::DecodedTextureCache,
                SceneResourceRootKind::ManagedLibrary,
                SceneResourceRootKind::ScenePackageArchive,
                SceneResourceRootKind::BuiltinAssets,
            ]
        );
        assert!(!roots[4].searchable);
        assert!(roots[4].exists);
    }

    #[test]
    fn resource_roots_insert_external_assets_before_builtin_assets_when_configured() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(managed_root.join("decoded")).expect("decoded dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::create_dir_all(&external_root).expect("external dir");

        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed_root,
            &builtin_root,
            Some(external_root.clone()),
        );
        let roots = resolver.resource_roots();

        assert_eq!(
            roots.iter().map(|root| root.kind).collect::<Vec<_>>(),
            vec![
                SceneResourceRootKind::ExtractedContent,
                SceneResourceRootKind::SourceContent,
                SceneResourceRootKind::DecodedTextureCache,
                SceneResourceRootKind::ManagedLibrary,
                SceneResourceRootKind::ScenePackageArchive,
                SceneResourceRootKind::ExternalAssets,
                SceneResourceRootKind::BuiltinAssets,
            ]
        );
        assert_eq!(roots[5].path, external_root);
        assert!(roots[5].searchable);
    }

    #[test]
    fn relative_path_resolution_prefers_scene_roots_before_builtin_assets() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(managed_root.join("decoded")).expect("decoded dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        fs::write(
            managed_root.join("extracted").join("asset.txt"),
            b"extracted",
        )
        .expect("extracted asset");
        fs::write(
            managed_root.join("source").join("source-only.txt"),
            b"source",
        )
        .expect("source asset");
        fs::write(
            managed_root.join("decoded").join("decoded-only.txt"),
            b"decoded",
        )
        .expect("decoded asset");
        fs::write(managed_root.join("managed-only.txt"), b"managed").expect("managed asset");
        fs::write(builtin_root.join("builtin-only.txt"), b"builtin").expect("builtin asset");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        assert_eq!(
            resolver.resolve_relative_path("asset.txt"),
            Some(managed_root.join("extracted").join("asset.txt"))
        );
        assert_eq!(
            resolver.resolve_relative_path("source-only.txt"),
            Some(managed_root.join("source").join("source-only.txt"))
        );
        assert_eq!(
            resolver.resolve_relative_path("decoded-only.txt"),
            Some(managed_root.join("decoded").join("decoded-only.txt"))
        );
        assert_eq!(
            resolver.resolve_relative_path("managed-only.txt"),
            Some(managed_root.join("managed-only.txt"))
        );
        assert_eq!(
            resolver.resolve_relative_path("builtin-only.txt"),
            Some(builtin_root.join("builtin-only.txt"))
        );
    }

    #[test]
    fn relative_path_resolution_prefers_external_assets_before_builtin_assets() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(managed_root.join("decoded")).expect("decoded dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::create_dir_all(&external_root).expect("external dir");
        fs::create_dir_all(external_root.join("assets/materials/compat"))
            .expect("external compat dir");
        fs::create_dir_all(builtin_root.join("assets/materials/compat"))
            .expect("builtin compat dir");

        fs::write(
            external_root.join("assets/materials/compat/default.material"),
            b"external",
        )
        .expect("external material");
        fs::write(
            builtin_root.join("assets/materials/compat/default.material"),
            b"builtin",
        )
        .expect("builtin material");

        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed_root,
            &builtin_root,
            Some(external_root.clone()),
        );

        assert_eq!(
            resolver.resolve_relative_path("assets/materials/compat/default.material"),
            Some(external_root.join("assets/materials/compat/default.material"))
        );
    }

    #[test]
    fn relative_path_resolution_rejects_directories_that_only_match_by_name() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(&extracted_root).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::create_dir_all(extracted_root.join("effects/custom/tint")).expect("shader dir");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        assert_eq!(resolver.resolve_relative_path("effects/custom/tint"), None);
    }

    #[test]
    fn texture_candidates_include_material_relative_tex_and_decoded_outputs() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        let decoded_root = managed_root.join("decoded");

        fs::create_dir_all(extracted_root.join("materials")).expect("materials dir");
        fs::create_dir_all(decoded_root.join("materials")).expect("decoded materials");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        let raw_texture = extracted_root
            .join("materials")
            .join("hero")
            .with_extension("tex");
        fs::write(&raw_texture, b"tex").expect("raw texture");
        fs::write(
            decoded_root
                .join("materials")
                .join("hero")
                .with_extension("png"),
            b"decoded",
        )
        .expect("decoded texture");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let candidates =
            resolver.resolve_texture_candidates(Some("materials/hero.material"), "hero");

        assert_eq!(
            candidates.first(),
            Some(
                &decoded_root
                    .join("materials")
                    .join("hero")
                    .with_extension("png"),
            )
        );
        assert!(candidates.contains(&raw_texture));
        assert!(candidates.contains(
            &decoded_root
                .join("materials")
                .join("hero")
                .with_extension("png"),
        ));
    }

    #[test]
    fn texture_candidates_accept_authored_image_and_tex_json_sidecar_references() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        fs::create_dir_all(extracted_root.join("materials/effects")).expect("effects dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        let tex_path = extracted_root.join("materials/effects/waternormal.tex");
        fs::write(&tex_path, b"tex").expect("texture");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        for authored in [
            "materials/effects/waternormal.png",
            "materials/effects/waternormal.tex-json",
            "materials/effects/waternormal.tex.json",
        ] {
            let candidates = resolver.resolve_texture_candidates(None, authored);
            assert!(
                candidates.contains(&tex_path),
                "{authored} should resolve to the .tex sidecar"
            );
        }
    }

    #[test]
    fn effect_package_lookup_falls_back_to_equivalent_external_assets_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");
        let extracted_effect_root = managed_root.join("extracted").join("effects/pulse");
        let external_material_path =
            external_root.join("effects/pulse/materials/effects/pulse.json");

        fs::create_dir_all(&extracted_effect_root).expect("effect root");
        fs::create_dir_all(external_material_path.parent().expect("material parent"))
            .expect("external material parent");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(&external_material_path, b"pulse").expect("external material");

        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed_root,
            &builtin_root,
            Some(external_root),
        );
        let lookup = resolver.inspect_relative_path_with_local_root(
            "materials/effects/pulse.json",
            SceneResourceRootKind::EffectPackage,
            &extracted_effect_root,
        );

        assert_eq!(lookup.matched_path, Some(external_material_path.clone()));
        assert_eq!(
            lookup.matched_root_kind,
            Some(SceneResourceRootKind::ExternalAssets)
        );
        assert!(lookup
            .attempted_candidates
            .contains(&external_material_path));
    }

    #[test]
    fn effect_package_lookup_preserves_builtin_assets_provenance() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_effect_root = managed_root.join("extracted").join("effects/pulse");
        let builtin_material_path = builtin_root.join("effects/pulse/materials/effects/pulse.json");

        fs::create_dir_all(&extracted_effect_root).expect("effect root");
        fs::create_dir_all(builtin_material_path.parent().expect("material parent"))
            .expect("builtin material parent");
        fs::write(&builtin_material_path, b"pulse").expect("builtin material");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let lookup = resolver.inspect_relative_path_with_local_root(
            "materials/effects/pulse.json",
            SceneResourceRootKind::EffectPackage,
            &extracted_effect_root,
        );

        assert_eq!(lookup.matched_path, Some(builtin_material_path));
        assert_eq!(
            lookup.matched_root_kind,
            Some(SceneResourceRootKind::BuiltinAssets)
        );
    }

    #[test]
    fn shader_source_lookup_resolves_authored_pairs_from_effect_packages_and_scene_roots() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");
        let extracted_root = managed_root.join("extracted");
        let extracted_effect_root =
            extracted_root.join("effects/workshop/2973943998/iris_movement__");
        let external_effect_root = external_root.join("effects/pulse");
        let external_vert = external_effect_root.join("shaders/effects/pulse.vert");
        let external_frag = external_effect_root.join("shaders/effects/pulse.frag");
        let extracted_vert =
            extracted_root.join("shaders/workshop/2973943998/effects/iris_movement__.vert");
        let extracted_frag =
            extracted_root.join("shaders/workshop/2973943998/effects/iris_movement__.frag");

        fs::create_dir_all(&extracted_effect_root).expect("extracted effect root");
        fs::create_dir_all(external_vert.parent().expect("external vert parent"))
            .expect("external vert parent dir");
        fs::create_dir_all(extracted_vert.parent().expect("extracted vert parent"))
            .expect("extracted vert parent dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(&external_vert, b"vert").expect("external vert");
        fs::write(&external_frag, b"frag").expect("external frag");
        fs::write(&extracted_vert, b"vert").expect("extracted vert");
        fs::write(&extracted_frag, b"frag").expect("extracted frag");

        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed_root,
            &builtin_root,
            Some(external_root),
        );

        let external_lookup = resolver.inspect_shader_source_with_local_root(
            "effects/pulse",
            SceneResourceRootKind::EffectPackage,
            extracted_root.join("effects/pulse"),
        );
        assert_eq!(
            external_lookup.kind,
            super::SceneShaderSourceKind::AuthoredSourceSet
        );
        assert!(external_lookup.matched_paths.contains(&external_vert));
        assert!(external_lookup.matched_paths.contains(&external_frag));
        assert_eq!(
            external_lookup.lookup.matched_root_kind,
            Some(SceneResourceRootKind::ExternalAssets)
        );

        let workshop_lookup = resolver.inspect_shader_source_with_local_root(
            "workshop/2973943998/effects/iris_movement__",
            SceneResourceRootKind::EffectPackage,
            &extracted_effect_root,
        );
        assert_eq!(
            workshop_lookup.kind,
            super::SceneShaderSourceKind::AuthoredSourceSet
        );
        assert!(workshop_lookup.matched_paths.contains(&extracted_vert));
        assert!(workshop_lookup.matched_paths.contains(&extracted_frag));
        assert_eq!(
            workshop_lookup.lookup.matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );
    }

    #[test]
    fn builtin_assets_root_resolves_phase_10_compat_resources() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = default_builtin_scene_assets_root();

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(managed_root.join("decoded")).expect("decoded dir");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        assert_eq!(
            resolver.resolve_relative_path("assets/shaders/compat/scene-model.metal"),
            Some(builtin_root.join("assets/shaders/compat/scene-model.metal"))
        );
        assert_eq!(
            resolver.resolve_relative_path("assets/materials/compat/default.material"),
            Some(builtin_root.join("assets/materials/compat/default.material"))
        );
        assert_eq!(
            resolver.resolve_relative_path("assets/effects/compat/copy.effect"),
            Some(builtin_root.join("assets/effects/compat/copy.effect"))
        );
    }

    #[test]
    fn resolves_text_font_candidates_from_builtin_assets_and_family_reference() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let builtin_font = builtin_root.join("assets").join("fonts").join("clock.ttf");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(builtin_font.parent().expect("builtin font dir"))
            .expect("builtin font parent");
        fs::write(&builtin_font, b"font").expect("builtin font");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let resolved = resolver.resolve_text_font("clock");

        assert_eq!(resolved.authored_reference, "clock");
        assert_eq!(
            resolved.reference_kind,
            SceneTextFontReferenceKind::FamilyLike
        );
        assert_eq!(resolved.file_candidates, vec![builtin_font]);
        assert!(resolved.family_candidates.contains(&"clock".to_string()));
        assert!(resolved.family_candidates.contains(&"Clock".to_string()));
        assert!(resolved.cache_key.starts_with("font:"));
    }

    #[test]
    fn text_font_resolution_prefers_scene_then_external_then_builtin_assets() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");
        let extracted_font = managed_root
            .join("extracted")
            .join("assets/fonts/title.otf");
        let external_font = external_root.join("assets/fonts/title.otf");
        let builtin_font = builtin_root.join("assets/fonts/title.otf");

        fs::create_dir_all(extracted_font.parent().expect("extracted font parent"))
            .expect("extracted font dir");
        fs::create_dir_all(external_font.parent().expect("external font parent"))
            .expect("external font dir");
        fs::create_dir_all(builtin_font.parent().expect("builtin font parent"))
            .expect("builtin font dir");
        fs::write(&extracted_font, b"scene").expect("scene font");
        fs::write(&external_font, b"external").expect("external font");
        fs::write(&builtin_font, b"builtin").expect("builtin font");

        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed_root,
            &builtin_root,
            Some(external_root),
        );

        assert_eq!(
            resolver.resolve_text_font("title").file_candidates.first(),
            Some(&extracted_font)
        );

        fs::remove_file(&extracted_font).expect("remove extracted font");
        assert_eq!(
            resolver.resolve_text_font("title").file_candidates.first(),
            Some(&external_font)
        );
    }

    #[test]
    fn preserves_authored_font_stem_when_custom_font_file_is_missing() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let resolved = resolver.resolve_text_font("fonts/Alcubierre.otf");

        assert!(resolved.file_candidates.is_empty());
        assert!(resolved
            .family_candidates
            .contains(&"Alcubierre".to_string()));
    }

    #[test]
    fn maps_system_font_aliases_to_real_family_candidates() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");

        fs::create_dir_all(managed_root.join("source")).expect("source dir");
        fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let resolved = resolver.resolve_text_font("systemfont_comicsans");

        assert_eq!(
            resolved.reference_kind,
            SceneTextFontReferenceKind::SystemFontAlias
        );
        assert!(resolved
            .family_candidates
            .contains(&"Comic Sans MS".to_string()));
    }

    #[test]
    fn detects_path_like_font_references() {
        assert!(font_reference_looks_like_path("fonts/clock.ttf"));
        assert!(font_reference_looks_like_path("fonts/clock.woff2"));
        assert!(font_reference_looks_like_path("clock.otf"));
        assert!(!font_reference_looks_like_path("DIN Alternate"));
        assert_eq!(
            scene_text_font_reference_kind("systemfont_arial"),
            Some(SceneTextFontReferenceKind::SystemFontAlias)
        );
    }

    #[test]
    fn builtin_assets_root_accepts_direct_scene_resource_dir() {
        let temp = tempdir().expect("temp dir");
        let resource_dir = temp.path().join("Resources");
        let scene_dir = resource_dir.join("scene");
        fs::create_dir_all(&scene_dir).expect("scene dir");

        assert_eq!(
            super::builtin_scene_assets_root_from_resource_dir(&resource_dir),
            Some(scene_dir)
        );
    }

    #[test]
    fn builtin_assets_root_accepts_nested_resources_scene_dir() {
        let temp = tempdir().expect("temp dir");
        let resource_dir = temp.path().join("Resources");
        let scene_dir = resource_dir.join("resources").join("scene");
        fs::create_dir_all(&scene_dir).expect("nested scene dir");

        assert_eq!(
            super::builtin_scene_assets_root_from_resource_dir(&resource_dir),
            Some(scene_dir)
        );
    }
}
