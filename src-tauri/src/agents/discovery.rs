use std::path::Path;

use super::{AgentId, AgentInstallation, InstallationId};

/// Why an adapter considered a configuration home during this discovery run.
/// Evidence is diagnostic-only and is never persisted with an installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryEvidence {
    DefaultHome,
    Environment,
    Process,
    UserConfirmed,
}

/// Adapter-owned identity used only while merging discovery candidates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalInstallationKey {
    agent_id: AgentId,
    config_home: String,
}

/// Validated discovery result before registry-level deduplication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationCandidate {
    canonical_key: CanonicalInstallationKey,
    installation: AgentInstallation,
    evidence: DiscoveryEvidence,
}

impl InstallationCandidate {
    pub fn from_existing_home(
        agent_id: impl Into<AgentId>,
        config_home: impl AsRef<Path>,
        evidence: DiscoveryEvidence,
    ) -> Option<Self> {
        let agent_id = agent_id.into();
        let canonical_home = std::fs::canonicalize(config_home).ok()?;
        if !canonical_home.is_dir() {
            return None;
        }

        let config_home = canonical_home.to_string_lossy().into_owned();
        let canonical_key = CanonicalInstallationKey {
            agent_id: agent_id.clone(),
            config_home: config_home.clone(),
        };
        let installation_id = InstallationId::from(format!("{}:{config_home}", agent_id.as_str()));
        let installation = AgentInstallation::with_id(installation_id, agent_id, config_home);

        Some(Self {
            canonical_key,
            installation,
            evidence,
        })
    }

    pub fn installation(&self) -> &AgentInstallation {
        &self.installation
    }

    pub fn evidence(&self) -> DiscoveryEvidence {
        self.evidence
    }

    pub(crate) fn canonical_key(&self) -> &CanonicalInstallationKey {
        &self.canonical_key
    }

    pub(crate) fn into_installation(self) -> AgentInstallation {
        self.installation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_candidate_normalizes_trailing_separators() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("config-home");
        std::fs::create_dir_all(&home).unwrap();

        let direct = InstallationCandidate::from_existing_home(
            "codex",
            &home,
            DiscoveryEvidence::DefaultHome,
        )
        .unwrap();
        let trailing = InstallationCandidate::from_existing_home(
            "codex",
            format!("{}/", home.display()),
            DiscoveryEvidence::Environment,
        )
        .unwrap();

        assert_eq!(direct.canonical_key(), trailing.canonical_key());
        assert_eq!(direct.installation().id, trailing.installation().id);
    }

    #[test]
    fn missing_config_homes_are_not_candidates() {
        let candidate = InstallationCandidate::from_existing_home(
            "codex",
            "/definitely/missing/ad-codex-home",
            DiscoveryEvidence::Environment,
        );

        assert!(candidate.is_none());
    }
}
