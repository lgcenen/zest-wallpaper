use serde::{Deserialize, Serialize};

use crate::services::scene_resource_service::{
    SceneResourceLookup, SceneResourceRoot, SceneResourceRootKind, SceneTextFontLookup,
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum SceneDiagnosticSeverity {
    Warning,
    Fatal,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SceneDiagnosticCategory {
    Resource,
    Capability,
    Runtime,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SceneDiagnosticDomain {
    Scene,
    Visual,
    Text,
    Audio,
    Sound,
    Particle,
    Input,
    VideoTexture,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneDiagnosticResourceDetail {
    pub authored_reference: String,
    pub attempted_roots: Vec<SceneResourceRoot>,
    pub attempted_candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_root_kind: Option<SceneResourceRootKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_path: Option<String>,
    pub external_assets_available: bool,
    pub builtin_assets_available: bool,
    pub reference_resolved: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub present_but_unsupported: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub family_candidates: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneDiagnosticDetail {
    pub category: SceneDiagnosticCategory,
    pub domain: SceneDiagnosticDomain,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<SceneDiagnosticResourceDetail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underlying_diagnostic: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneDiagnosticEntry {
    pub severity: SceneDiagnosticSeverity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<SceneDiagnosticDetail>,
}

impl SceneDiagnosticEntry {
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(SceneDiagnosticSeverity::Warning, code, message)
    }

    pub fn fatal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(SceneDiagnosticSeverity::Fatal, code, message)
    }

    pub fn new(
        severity: SceneDiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            object_id: None,
            object_name: None,
            object_kind: None,
            resource_path: None,
            detail: None,
        }
    }

    pub fn with_object(
        mut self,
        object_id: Option<u32>,
        object_name: Option<impl Into<String>>,
        object_kind: Option<impl Into<String>>,
    ) -> Self {
        self.object_id = object_id;
        self.object_name = object_name.map(Into::into);
        self.object_kind = object_kind.map(Into::into);
        self
    }

    pub fn with_resource_path(mut self, resource_path: Option<impl Into<String>>) -> Self {
        self.resource_path = resource_path.map(Into::into);
        self
    }

    pub fn with_detail(mut self, detail: SceneDiagnosticDetail) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl SceneDiagnosticDetail {
    pub fn resource(
        domain: SceneDiagnosticDomain,
        resource: SceneDiagnosticResourceDetail,
    ) -> Self {
        Self {
            category: SceneDiagnosticCategory::Resource,
            domain,
            resource: Some(resource),
            runtime_stage: None,
            reason: None,
            underlying_diagnostic: None,
            notes: Vec::new(),
        }
    }

    pub fn runtime(
        domain: SceneDiagnosticDomain,
        runtime_stage: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            category: SceneDiagnosticCategory::Runtime,
            domain,
            resource: None,
            runtime_stage: Some(runtime_stage.into()),
            reason: Some(reason.into()),
            underlying_diagnostic: None,
            notes: Vec::new(),
        }
    }

    pub fn capability(domain: SceneDiagnosticDomain, reason: impl Into<String>) -> Self {
        Self {
            category: SceneDiagnosticCategory::Capability,
            domain,
            resource: None,
            runtime_stage: None,
            reason: Some(reason.into()),
            underlying_diagnostic: None,
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_underlying_diagnostic(mut self, value: impl Into<String>) -> Self {
        self.underlying_diagnostic = Some(value.into());
        self
    }
}

impl SceneDiagnosticResourceDetail {
    pub fn from_lookup(lookup: &SceneResourceLookup) -> Self {
        Self {
            authored_reference: lookup.authored_reference.clone(),
            attempted_roots: lookup.attempted_roots.clone(),
            attempted_candidates: lookup
                .attempted_candidates
                .iter()
                .map(|candidate| candidate.display().to_string())
                .collect(),
            matched_root_kind: lookup.matched_root_kind,
            matched_path: lookup
                .matched_path
                .as_ref()
                .map(|path| path.display().to_string()),
            external_assets_available: lookup.external_assets_available,
            builtin_assets_available: lookup.builtin_assets_available,
            reference_resolved: lookup.matched_path.is_some(),
            present_but_unsupported: false,
            family_candidates: Vec::new(),
        }
    }

    pub fn from_text_font_lookup(lookup: &SceneTextFontLookup) -> Self {
        let mut detail = Self::from_lookup(&lookup.lookup);
        detail.family_candidates = lookup.family_candidates.clone();
        detail
    }

    pub fn mark_present_but_unsupported(mut self) -> Self {
        self.present_but_unsupported = true;
        self
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}
