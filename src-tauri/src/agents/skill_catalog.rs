use std::path::{Component, Path};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::execution_state::{ExecutionState, StateDirectory};
use super::{ContentDigest, SkillArtifactRef};
use crate::models::{SkillSource, SkillSourceType};

const SKILL_CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillSourceRequest {
    pub display_name: String,
    pub source_type: SkillSourceType,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCatalogEntry {
    pub source_id: String,
    pub display_name: String,
    pub source_type: SkillSourceType,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
    pub current_artifact: SkillArtifactRef,
    pub added_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillCatalogSnapshot {
    pub schema_version: u32,
    pub revision: ContentDigest,
    pub entries: Vec<SkillCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SkillCatalogDocument {
    schema_version: u32,
    entries: Vec<SkillCatalogEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillCatalogError {
    #[error("invalid Skill catalog request: {0}")]
    InvalidRequest(String),
    #[error("Skill catalog source was not found: {0}")]
    NotFound(String),
    #[error("Skill catalog is corrupt: {0}")]
    Corrupt(String),
    #[error("Skill catalog I/O failed at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) struct SkillCatalogState {
    pub(crate) document: SkillCatalogDocument,
    pub(crate) revision: ContentDigest,
    pub(crate) bytes: Option<Vec<u8>>,
}

pub fn load_skill_catalog_snapshot() -> Result<SkillCatalogSnapshot, SkillCatalogError> {
    let execution_state = ExecutionState::open().map_err(|source| SkillCatalogError::Io {
        path: "~/.ad".into(),
        source,
    })?;
    let state = load_skill_catalog_state_from(execution_state.state())?;
    Ok(state.snapshot())
}

pub(crate) fn load_skill_catalog_state() -> Result<SkillCatalogState, SkillCatalogError> {
    let execution_state = ExecutionState::open().map_err(|source| SkillCatalogError::Io {
        path: "~/.ad".into(),
        source,
    })?;
    load_skill_catalog_state_from(execution_state.state())
}

pub(super) fn load_skill_catalog_state_from(
    directory: &StateDirectory,
) -> Result<SkillCatalogState, SkillCatalogError> {
    let bytes = match directory.read("skill_catalog.json") {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let document = SkillCatalogDocument::empty();
            return Ok(SkillCatalogState {
                revision: document.revision()?,
                document,
                bytes: None,
            });
        }
        Err(source) => {
            return Err(SkillCatalogError::Io {
                path: directory
                    .display_path()
                    .join("skill_catalog.json")
                    .display()
                    .to_string(),
                source,
            });
        }
    };
    let document: SkillCatalogDocument = serde_json::from_slice(&bytes)
        .map_err(|error| SkillCatalogError::Corrupt(error.to_string()))?;
    document.validate()?;
    Ok(SkillCatalogState {
        revision: ContentDigest::sha256(&bytes),
        document,
        bytes: Some(bytes),
    })
}

impl SkillCatalogState {
    pub(crate) fn snapshot(&self) -> SkillCatalogSnapshot {
        SkillCatalogSnapshot {
            schema_version: SKILL_CATALOG_SCHEMA_VERSION,
            revision: self.revision.clone(),
            entries: self.document.entries.clone(),
        }
    }
}

impl SkillCatalogDocument {
    pub(crate) fn empty() -> Self {
        Self {
            schema_version: SKILL_CATALOG_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }

    pub(crate) fn render(&self) -> Result<Vec<u8>, SkillCatalogError> {
        self.validate()?;
        serde_json::to_vec_pretty(self)
            .map_err(|error| SkillCatalogError::Corrupt(error.to_string()))
    }

    pub(crate) fn revision(&self) -> Result<ContentDigest, SkillCatalogError> {
        Ok(ContentDigest::sha256(&self.render()?))
    }

    pub(crate) fn entry(&self, source_id: &str) -> Option<&SkillCatalogEntry> {
        self.entries
            .iter()
            .find(|entry| entry.source_id == source_id)
    }

    pub(crate) fn add(
        &mut self,
        source_id: String,
        request: &SkillSourceRequest,
        artifact: SkillArtifactRef,
        now: DateTime<Utc>,
    ) -> Result<SkillCatalogEntry, SkillCatalogError> {
        validate_request(request)?;
        if self.entries.iter().any(|entry| {
            entry
                .display_name
                .eq_ignore_ascii_case(&request.display_name)
                || same_source_location(entry, request)
        }) {
            return Err(SkillCatalogError::InvalidRequest(
                "a source with the same name or location already exists".into(),
            ));
        }
        validate_source_id(&source_id)?;
        let entry = SkillCatalogEntry {
            source_id,
            display_name: request.display_name.trim().to_owned(),
            source_type: request.source_type,
            location: request.location.clone(),
            branch: request.branch.clone(),
            subdirectory: request.subdirectory.clone(),
            auto_update: request.auto_update,
            current_artifact: artifact,
            added_at: now,
            updated_at: now,
        };
        self.entries.push(entry.clone());
        self.sort();
        self.validate()?;
        Ok(entry)
    }

    pub(crate) fn update_artifact(
        &mut self,
        source_id: &str,
        artifact: SkillArtifactRef,
        now: DateTime<Utc>,
    ) -> Result<SkillCatalogEntry, SkillCatalogError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.source_id == source_id)
            .ok_or_else(|| SkillCatalogError::NotFound(source_id.to_owned()))?;
        entry.current_artifact = artifact;
        entry.updated_at = now;
        let updated = entry.clone();
        self.validate()?;
        Ok(updated)
    }

    pub(crate) fn remove(
        &mut self,
        source_id: &str,
    ) -> Result<SkillCatalogEntry, SkillCatalogError> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.source_id == source_id)
            .ok_or_else(|| SkillCatalogError::NotFound(source_id.to_owned()))?;
        Ok(self.entries.remove(index))
    }

    fn validate(&self) -> Result<(), SkillCatalogError> {
        if self.schema_version != SKILL_CATALOG_SCHEMA_VERSION {
            return Err(SkillCatalogError::Corrupt(format!(
                "unsupported schema version {}",
                self.schema_version
            )));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for entry in &self.entries {
            validate_source_id(&entry.source_id)
                .map_err(|error| SkillCatalogError::Corrupt(error.to_string()))?;
            validate_request(&entry.request())
                .map_err(|error| SkillCatalogError::Corrupt(error.to_string()))?;
            if !ids.insert(entry.source_id.clone())
                || !names.insert(entry.display_name.to_ascii_lowercase())
            {
                return Err(SkillCatalogError::Corrupt(
                    "catalog contains duplicate source identity".into(),
                ));
            }
            if entry.current_artifact.source_id != entry.source_id {
                return Err(SkillCatalogError::Corrupt(
                    "artifact provenance does not match its catalog source".into(),
                ));
            }
        }
        Ok(())
    }

    fn sort(&mut self) {
        self.entries.sort_by(|left, right| {
            left.display_name
                .to_ascii_lowercase()
                .cmp(&right.display_name.to_ascii_lowercase())
                .then_with(|| left.source_id.cmp(&right.source_id))
        });
    }
}

impl SkillCatalogEntry {
    pub(crate) fn request(&self) -> SkillSourceRequest {
        SkillSourceRequest {
            display_name: self.display_name.clone(),
            source_type: self.source_type,
            location: self.location.clone(),
            branch: self.branch.clone(),
            subdirectory: self.subdirectory.clone(),
            auto_update: self.auto_update,
        }
    }

    pub(crate) fn acquisition_source(&self) -> SkillSource {
        SkillSource {
            id: self.source_id.clone(),
            source_type: self.source_type,
            url: self.location.clone(),
            branch: self.branch.clone(),
            subdirectory: self.subdirectory.clone(),
            auto_update: self.auto_update,
            added_at: self.added_at,
        }
    }
}

pub(crate) fn acquisition_source(
    source_id: String,
    request: &SkillSourceRequest,
) -> Result<SkillSource, SkillCatalogError> {
    validate_source_id(&source_id)?;
    validate_request(request)?;
    Ok(SkillSource {
        id: source_id,
        source_type: request.source_type,
        url: request.location.clone(),
        branch: request.branch.clone(),
        subdirectory: request.subdirectory.clone(),
        auto_update: request.auto_update,
        added_at: Utc::now(),
    })
}

pub(crate) fn new_source_id() -> String {
    format!("skill-source:{}", uuid::Uuid::new_v4())
}

pub(crate) fn validate_request(request: &SkillSourceRequest) -> Result<(), SkillCatalogError> {
    let name = request.display_name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(SkillCatalogError::InvalidRequest(
            "source name must contain 1 to 100 visible characters".into(),
        ));
    }
    if request.location.trim() != request.location || request.location.is_empty() {
        return Err(SkillCatalogError::InvalidRequest(
            "source location must not be empty or padded".into(),
        ));
    }
    if request.source_type == SkillSourceType::Local {
        if request.branch.is_some() {
            return Err(SkillCatalogError::InvalidRequest(
                "local sources cannot declare a Git branch".into(),
            ));
        }
        let location = Path::new(&request.location);
        if !location.is_absolute() {
            return Err(SkillCatalogError::InvalidRequest(
                "local source path must be absolute".into(),
            ));
        }
    }
    if let Some(subdirectory) = request.subdirectory.as_deref() {
        let path = Path::new(subdirectory);
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(SkillCatalogError::InvalidRequest(
                "source subdirectory must be a safe relative path".into(),
            ));
        }
    }
    Ok(())
}

fn validate_source_id(source_id: &str) -> Result<(), SkillCatalogError> {
    let Some(value) = source_id.strip_prefix("skill-source:") else {
        return Err(SkillCatalogError::InvalidRequest(
            "source id is not backend-issued".into(),
        ));
    };
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| SkillCatalogError::InvalidRequest("source id is invalid".into()))
}

fn same_source_location(entry: &SkillCatalogEntry, request: &SkillSourceRequest) -> bool {
    entry.source_type == request.source_type
        && entry.location == request.location
        && entry.branch == request.branch
        && entry.subdirectory == request.subdirectory
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(source_id: &str) -> SkillArtifactRef {
        SkillArtifactRef {
            schema_version: 1,
            artifact_id: "skill-artifact:sha256:abc".into(),
            source_id: source_id.into(),
            source_revision: "local:sha256:abc".into(),
            tree_digest: ContentDigest::from("sha256:abc"),
            manifest_digest: ContentDigest::from("sha256:def"),
            skills: Vec::new(),
            activation_impact: super::super::SkillActivationImpact {
                instructions: Vec::new(),
                hooks: Vec::new(),
                mcp: Vec::new(),
                commands: Vec::new(),
                scripts: Vec::new(),
                binaries: Vec::new(),
                executable_paths: Vec::new(),
                digest: ContentDigest::from("sha256:impact"),
            },
        }
    }

    #[test]
    fn catalog_ids_are_backend_issued_and_artifact_provenance_is_bound() {
        let mut catalog = SkillCatalogDocument::empty();
        let source_id = new_source_id();
        let request = SkillSourceRequest {
            display_name: "Review".into(),
            source_type: SkillSourceType::Local,
            location: "/tmp/review".into(),
            branch: None,
            subdirectory: None,
            auto_update: false,
        };
        catalog
            .add(
                source_id.clone(),
                &request,
                artifact(&source_id),
                Utc::now(),
            )
            .unwrap();

        assert!(catalog
            .add(
                "../escape".into(),
                &request,
                artifact("../escape"),
                Utc::now()
            )
            .is_err());
        assert!(catalog.render().is_ok());
    }

    #[test]
    fn source_remove_only_changes_catalog_bytes() {
        let mut catalog = SkillCatalogDocument::empty();
        let source_id = new_source_id();
        let request = SkillSourceRequest {
            display_name: "Review".into(),
            source_type: SkillSourceType::Local,
            location: "/tmp/review".into(),
            branch: None,
            subdirectory: None,
            auto_update: false,
        };
        catalog
            .add(
                source_id.clone(),
                &request,
                artifact(&source_id),
                Utc::now(),
            )
            .unwrap();
        let removed = catalog.remove(&source_id).unwrap();

        assert_eq!(
            removed.current_artifact.artifact_id,
            "skill-artifact:sha256:abc"
        );
        assert!(catalog.entries.is_empty());
    }
}
