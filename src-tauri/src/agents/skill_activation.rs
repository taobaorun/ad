use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::skill_artifact_tree::{TreeEntryKind, TreeManifest};
use super::{ContentDigest, SkillActivationImpact, SkillArtifactError, SkillArtifactItem};

pub(super) fn inspect_skills(
    tree_root: &Path,
    manifest: &TreeManifest,
) -> Result<Vec<SkillArtifactItem>, SkillArtifactError> {
    let mut skills = Vec::new();
    for entry in &manifest.entries {
        if entry.kind != TreeEntryKind::File
            || Path::new(&entry.path)
                .file_name()
                .and_then(|name| name.to_str())
                != Some("SKILL.md")
        {
            continue;
        }
        let parent = Path::new(&entry.path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let logical_id = read_skill_name(&tree_root.join(&entry.path)).unwrap_or_else(|| {
            parent
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("root")
                .to_owned()
        });
        skills.push(SkillArtifactItem {
            logical_id,
            subpath: parent.to_string_lossy().into_owned(),
            instruction_digest: entry.content_digest.clone().ok_or_else(|| {
                SkillArtifactError::Corrupt(format!("{} has no content digest", entry.path))
            })?,
        });
    }
    skills.sort_by(|left, right| {
        left.logical_id
            .cmp(&right.logical_id)
            .then_with(|| left.subpath.cmp(&right.subpath))
    });
    Ok(skills)
}

pub(super) fn inspect_activation_impact(
    tree_root: &Path,
    manifest: &TreeManifest,
) -> Result<SkillActivationImpact, SkillArtifactError> {
    let mut impact = SkillActivationImpact {
        instructions: Vec::new(),
        hooks: Vec::new(),
        mcp: Vec::new(),
        commands: Vec::new(),
        scripts: Vec::new(),
        binaries: Vec::new(),
        executable_paths: Vec::new(),
        digest: ContentDigest::sha256(&[]),
    };
    for entry in &manifest.entries {
        if entry.kind != TreeEntryKind::File {
            continue;
        }
        let path = Path::new(&entry.path);
        let components = path
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        let file_name = components.last().map(String::as_str).unwrap_or_default();
        if file_name == "skill.md" {
            impact.instructions.push(entry.path.clone());
        }
        if components.iter().any(|component| component == "hooks") {
            impact.hooks.push(entry.path.clone());
        }
        if components.iter().any(|component| component == "commands") {
            impact.commands.push(entry.path.clone());
        }
        if file_name == ".mcp.json" || components.iter().any(|component| component == "mcp") {
            impact.mcp.push(entry.path.clone());
        }
        if components
            .iter()
            .any(|component| matches!(component.as_str(), "scripts" | "bin"))
        {
            impact.scripts.push(entry.path.clone());
        }
        if entry.mode & 0o111 != 0 {
            impact.executable_paths.push(entry.path.clone());
        }
        if file_looks_binary(&tree_root.join(&entry.path))? {
            impact.binaries.push(entry.path.clone());
        }
    }
    let digest_input = serde_json::to_vec(&impact)
        .map_err(|error| SkillArtifactError::Corrupt(error.to_string()))?;
    impact.digest = ContentDigest::sha256(&digest_input);
    Ok(impact)
}

fn read_skill_name(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref().take(64 * 1024).read_to_end(&mut bytes).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let frontmatter = text.trim_start().strip_prefix("---")?;
    let end = frontmatter.find("---")?;
    frontmatter[..end].lines().find_map(|line| {
        line.trim()
            .strip_prefix("name:")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.trim_matches(['\"', '\'']).to_owned())
    })
}

fn file_looks_binary(path: &Path) -> Result<bool, SkillArtifactError> {
    let mut file = File::open(path).map_err(|source| io_error(path, source))?;
    let mut buffer = [0_u8; 8 * 1024];
    let read = file
        .read(&mut buffer)
        .map_err(|source| io_error(path, source))?;
    Ok(buffer[..read].contains(&0))
}

fn io_error(path: &Path, source: std::io::Error) -> SkillArtifactError {
    SkillArtifactError::Io {
        path: path.display().to_string(),
        source,
    }
}
