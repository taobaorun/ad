use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ContentDigest;

const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ArtifactLimits {
    pub(crate) max_entries: usize,
    pub(crate) max_depth: usize,
    pub(crate) max_file_bytes: u64,
    pub(crate) max_total_bytes: u64,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_entries: 4_096,
            max_depth: 32,
            max_file_bytes: 16 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TreeManifest {
    schema_version: u32,
    pub(crate) entries: Vec<TreeEntry>,
}

impl TreeManifest {
    pub(crate) fn digest(&self) -> Result<ContentDigest, ArtifactTreeError> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| ArtifactTreeError::Invalid(error.to_string()))?;
        Ok(ContentDigest::sha256(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TreeEntry {
    pub(crate) path: String,
    pub(crate) kind: TreeEntryKind,
    pub(crate) mode: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content_digest: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) symlink_target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TreeEntryKind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArtifactTreeError {
    #[error("invalid Skill artifact tree: {0}")]
    Invalid(String),
    #[error("Skill artifact tree exceeds {0}")]
    Budget(&'static str),
    #[error("Skill artifact I/O failed at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) fn inspect_tree(
    root: &Path,
    limits: ArtifactLimits,
) -> Result<TreeManifest, ArtifactTreeError> {
    inspect_tree_filtered(root, limits, &|_| true)
}

pub(crate) fn inspect_tree_filtered(
    root: &Path,
    limits: ArtifactLimits,
    include: &dyn Fn(&Path) -> bool,
) -> Result<TreeManifest, ArtifactTreeError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|source| io_error(root, source))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ArtifactTreeError::Invalid(
            "source root must be a physical directory".into(),
        ));
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|source| io_error(root, source))?;
    let mut state = InspectionState {
        root,
        canonical_root: &canonical_root,
        limits,
        entries: Vec::new(),
        total_bytes: 0,
    };
    inspect_directory(&mut state, Path::new(""), 0, include)?;
    Ok(TreeManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        entries: state.entries,
    })
}

pub(crate) fn copy_tree_verified(
    source: &Path,
    destination: &Path,
    expected: &TreeManifest,
    limits: ArtifactLimits,
) -> Result<(), ArtifactTreeError> {
    if destination.exists() || destination.is_symlink() {
        return Err(ArtifactTreeError::Invalid(
            "artifact staging destination already exists".into(),
        ));
    }
    std::fs::create_dir(destination).map_err(|source| io_error(destination, source))?;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700))
        .map_err(|source| io_error(destination, source))?;
    for entry in &expected.entries {
        let relative = safe_relative_path(&entry.path)?;
        let source_path = source.join(&relative);
        let destination_path = destination.join(&relative);
        match entry.kind {
            TreeEntryKind::Directory => {
                std::fs::create_dir(&destination_path)
                    .map_err(|source| io_error(&destination_path, source))?;
                std::fs::set_permissions(
                    &destination_path,
                    std::fs::Permissions::from_mode(entry.mode),
                )
                .map_err(|source| io_error(&destination_path, source))?;
            }
            TreeEntryKind::File => copy_file_checked(&source_path, &destination_path, entry)?,
            TreeEntryKind::Symlink => {
                let target = entry.symlink_target.as_deref().ok_or_else(|| {
                    ArtifactTreeError::Invalid(format!(
                        "symlink {} has no recorded target",
                        entry.path
                    ))
                })?;
                let current = std::fs::read_link(&source_path)
                    .map_err(|source| io_error(&source_path, source))?;
                if current != Path::new(target) {
                    return Err(ArtifactTreeError::Invalid(format!(
                        "source changed while copying {}",
                        entry.path
                    )));
                }
                std::os::unix::fs::symlink(target, &destination_path)
                    .map_err(|source| io_error(&destination_path, source))?;
            }
        }
    }
    let source_after = inspect_tree(source, limits)?;
    if &source_after != expected {
        return Err(ArtifactTreeError::Invalid(
            "source changed during artifact acquisition".into(),
        ));
    }
    let destination_manifest = inspect_tree(destination, limits)?;
    if &destination_manifest != expected {
        return Err(ArtifactTreeError::Invalid(
            "staged artifact does not match its source manifest".into(),
        ));
    }
    sync_tree(destination, expected)?;
    Ok(())
}

struct InspectionState<'a> {
    root: &'a Path,
    canonical_root: &'a Path,
    limits: ArtifactLimits,
    entries: Vec<TreeEntry>,
    total_bytes: u64,
}

fn inspect_directory(
    state: &mut InspectionState<'_>,
    relative: &Path,
    depth: usize,
    include: &dyn Fn(&Path) -> bool,
) -> Result<(), ArtifactTreeError> {
    if depth > state.limits.max_depth {
        return Err(ArtifactTreeError::Budget("maximum directory depth"));
    }
    let directory = state.root.join(relative);
    let mut children = std::fs::read_dir(&directory)
        .map_err(|source| io_error(&directory, source))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| io_error(&directory, source))?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let name = child.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| ArtifactTreeError::Invalid("tree contains a non-UTF-8 path".into()))?;
        if excluded_name(name) {
            continue;
        }
        let child_relative = relative.join(name);
        if !include(&child_relative) {
            continue;
        }
        let relative_text = normalized_relative(&child_relative)?;
        let path = child.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|source| io_error(&path, source))?;
        if state.entries.len() >= state.limits.max_entries {
            return Err(ArtifactTreeError::Budget("maximum entry count"));
        }
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path).map_err(|source| io_error(&path, source))?;
            validate_symlink(state, &path, &target)?;
            state.entries.push(TreeEntry {
                path: relative_text,
                kind: TreeEntryKind::Symlink,
                mode: 0,
                size: None,
                content_digest: None,
                symlink_target: Some(target.to_string_lossy().into_owned()),
            });
        } else if metadata.is_dir() {
            state.entries.push(TreeEntry {
                path: relative_text,
                kind: TreeEntryKind::Directory,
                mode: 0o755,
                size: None,
                content_digest: None,
                symlink_target: None,
            });
            inspect_directory(state, &child_relative, depth + 1, include)?;
        } else if metadata.is_file() {
            if metadata.nlink() != 1 {
                return Err(ArtifactTreeError::Invalid(format!(
                    "hard-linked file is not allowed: {}",
                    child_relative.display()
                )));
            }
            if metadata.len() > state.limits.max_file_bytes {
                return Err(ArtifactTreeError::Budget("maximum file size"));
            }
            state.total_bytes = state
                .total_bytes
                .checked_add(metadata.len())
                .ok_or(ArtifactTreeError::Budget("maximum total size"))?;
            if state.total_bytes > state.limits.max_total_bytes {
                return Err(ArtifactTreeError::Budget("maximum total size"));
            }
            let digest = digest_regular_file(&path, metadata.len())?;
            state.entries.push(TreeEntry {
                path: relative_text,
                kind: TreeEntryKind::File,
                mode: if metadata.mode() & 0o111 == 0 {
                    0o644
                } else {
                    0o755
                },
                size: Some(metadata.len()),
                content_digest: Some(digest),
                symlink_target: None,
            });
        } else {
            return Err(ArtifactTreeError::Invalid(format!(
                "special file is not allowed: {}",
                child_relative.display()
            )));
        }
    }
    Ok(())
}

fn validate_symlink(
    state: &InspectionState<'_>,
    path: &Path,
    target: &Path,
) -> Result<(), ArtifactTreeError> {
    if target.is_absolute() {
        return Err(ArtifactTreeError::Invalid(format!(
            "absolute symlink is not allowed: {}",
            path.display()
        )));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        ArtifactTreeError::Invalid(format!(
            "dangling or cyclic symlink is not allowed: {}",
            path.display()
        ))
    })?;
    let relative = canonical.strip_prefix(state.canonical_root).map_err(|_| {
        ArtifactTreeError::Invalid(format!(
            "symlink escapes its source tree: {}",
            path.display()
        ))
    })?;
    if relative.components().any(|component| {
        matches!(component, Component::Normal(name) if name.to_str().is_some_and(excluded_name))
    }) {
        return Err(ArtifactTreeError::Invalid(format!(
            "symlink points into an excluded directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn digest_regular_file(
    path: &Path,
    expected_size: u64,
) -> Result<ContentDigest, ArtifactTreeError> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(|source| io_error(path, source.into()))?;
    let stat = rustix::fs::fstat(&fd).map_err(|source| io_error(path, source.into()))?;
    if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file()
        || stat.st_nlink != 1
        || stat.st_size < 0
        || stat.st_size as u64 != expected_size
    {
        return Err(ArtifactTreeError::Invalid(format!(
            "source changed while reading {}",
            path.display()
        )));
    }
    let mut file = File::from(fd);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| io_error(path, source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(ContentDigest::from(format!(
        "sha256:{:x}",
        hasher.finalize()
    )))
}

fn copy_file_checked(
    source: &Path,
    destination: &Path,
    entry: &TreeEntry,
) -> Result<(), ArtifactTreeError> {
    let expected_size = entry
        .size
        .ok_or_else(|| ArtifactTreeError::Invalid("file entry has no size".into()))?;
    let expected_digest = entry
        .content_digest
        .as_ref()
        .ok_or_else(|| ArtifactTreeError::Invalid("file entry has no digest".into()))?;
    let source_file = rustix::fs::open(
        source,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(|source_error| io_error(source, source_error.into()))?;
    let stat = rustix::fs::fstat(&source_file)
        .map_err(|source_error| io_error(source, source_error.into()))?;
    if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file()
        || stat.st_nlink != 1
        || stat.st_size < 0
        || stat.st_size as u64 != expected_size
    {
        return Err(ArtifactTreeError::Invalid(format!(
            "source changed while copying {}",
            entry.path
        )));
    }
    let mut input = File::from(source_file);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .map_err(|source| io_error(destination, source))?;
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|source_error| io_error(source, source_error))?;
        if read == 0 {
            break;
        }
        copied += read as u64;
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|source| io_error(destination, source))?;
    }
    let actual = ContentDigest::from(format!("sha256:{:x}", hasher.finalize()));
    if copied != expected_size || &actual != expected_digest {
        return Err(ArtifactTreeError::Invalid(format!(
            "source changed while copying {}",
            entry.path
        )));
    }
    output
        .set_permissions(std::fs::Permissions::from_mode(entry.mode))
        .map_err(|source| io_error(destination, source))?;
    output
        .sync_all()
        .map_err(|source| io_error(destination, source))
}

fn sync_tree(root: &Path, manifest: &TreeManifest) -> Result<(), ArtifactTreeError> {
    let mut directories = manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == TreeEntryKind::Directory)
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));
    for relative in directories {
        File::open(root.join(relative))
            .and_then(|directory| directory.sync_all())
            .map_err(|source| io_error(&root.join(relative), source))?;
    }
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error(root, source))
}

fn excluded_name(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".cache"
            | ".pytest_cache"
            | ".venv"
            | "__pycache__"
            | "node_modules"
            | "target"
            | ".DS_Store"
    )
}

fn safe_relative_path(value: &str) -> Result<PathBuf, ArtifactTreeError> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || matches!(component, Component::ParentDir | Component::RootDir)
        })
    {
        return Err(ArtifactTreeError::Invalid(format!(
            "invalid artifact path: {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn normalized_relative(path: &Path) -> Result<String, ArtifactTreeError> {
    safe_relative_path(&path.to_string_lossy())?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ArtifactTreeError::Invalid("tree contains a non-UTF-8 path".into()))
}

fn io_error(path: &Path, source: std::io::Error) -> ArtifactTreeError {
    ArtifactTreeError::Io {
        path: path.display().to_string(),
        source,
    }
}
