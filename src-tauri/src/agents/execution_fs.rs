use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use super::{
    AgentError, AgentErrorCode, ContentDigest, ManagedResourceTarget, PlannedMutation,
    ResourceStateKind,
};

#[derive(Clone)]
pub(super) enum TargetState {
    Missing,
    File(Vec<u8>),
    Symlink(PathBuf),
    Directory(ContentDigest),
}

impl TargetState {
    pub(super) fn kind(&self) -> ResourceStateKind {
        match self {
            Self::Missing => ResourceStateKind::Missing,
            Self::File(_) => ResourceStateKind::File,
            Self::Symlink(_) => ResourceStateKind::Symlink,
            Self::Directory(_) => ResourceStateKind::Directory,
        }
    }

    pub(super) fn digest(&self) -> Option<ContentDigest> {
        match self {
            Self::Missing => None,
            Self::File(bytes) => Some(ContentDigest::sha256(bytes)),
            Self::Symlink(target) => {
                Some(ContentDigest::sha256(target.to_string_lossy().as_bytes()))
            }
            Self::Directory(digest) => Some(digest.clone()),
        }
    }
}

pub(super) fn observe_target(target: &ManagedResourceTarget) -> Result<TargetState, AgentError> {
    match std::fs::symlink_metadata(target.path()) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(target.path())
            .map(TargetState::Symlink)
            .map_err(|error| standalone_io_error(target.path(), error)),
        Ok(metadata) if metadata.is_file() => std::fs::read(target.path())
            .map(TargetState::File)
            .map_err(|error| standalone_io_error(target.path(), error)),
        Ok(metadata) if metadata.is_dir() => directory_tree_digest(target.path())
            .map(TargetState::Directory)
            .map_err(|error| standalone_io_error(target.path(), error)),
        Ok(_) => Err(AgentError {
            code: AgentErrorCode::PermissionDenied,
            message: format!(
                "Managed target is not a file, symlink, or directory: {}",
                target.path().display()
            ),
            agent_id: None,
            installation_id: None,
            resource: None,
            retryable: false,
            details: None,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TargetState::Missing),
        Err(error) => Err(standalone_io_error(target.path(), error)),
    }
}

pub(super) fn render_content(mutation: &PlannedMutation) -> Result<Vec<u8>, AgentError> {
    let content = mutation.content.as_ref().ok_or_else(|| AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: "Create and replace mutations require content".into(),
        agent_id: None,
        installation_id: Some(mutation.resource.installation_id.clone()),
        resource: Some(mutation.resource.clone()),
        retryable: false,
        details: None,
    })?;
    if mutation.media_type == "application/json" {
        serde_json::to_vec_pretty(content).map_err(|error| content_error(mutation, error))
    } else if let Some(value) = content.as_str() {
        Ok(value.as_bytes().to_vec())
    } else {
        serde_json::to_vec(content).map_err(|error| content_error(mutation, error))
    }
}

pub(super) fn remove_target(path: &Path) -> Result<(), AgentError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path).map_err(|error| standalone_io_error(path, error))
        }
        Ok(_) => std::fs::remove_file(path).map_err(|error| standalone_io_error(path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(standalone_io_error(path, error)),
    }
}

pub fn directory_tree_digest(root: &Path) -> Result<ContentDigest, std::io::Error> {
    let metadata = std::fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Directory source is not a physical directory: {}",
                root.display()
            ),
        ));
    }
    let mut encoded = Vec::new();
    digest_directory_entries(root, Path::new(""), &mut encoded)?;
    Ok(ContentDigest::sha256(&encoded))
}

pub(super) fn copy_directory_tree(source: &Path, target: &Path) -> Result<(), std::io::Error> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Directory source is not a physical directory: {}",
                source.display()
            ),
        ));
    }
    std::fs::create_dir(target)?;
    copy_directory_entries(source, target, Path::new(""))?;
    std::fs::set_permissions(
        target,
        std::fs::Permissions::from_mode(metadata.permissions().mode()),
    )
}

pub(super) fn write_directory_atomic(target: &Path, source: &Path) -> Result<(), std::io::Error> {
    write_directory_atomic_with_cleanup(target, source, remove_path)
}

fn write_directory_atomic_with_cleanup<F>(
    target: &Path,
    source: &Path,
    cleanup_previous: F,
) -> Result<(), std::io::Error>
where
    F: FnOnce(&Path) -> Result<(), std::io::Error>,
{
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Directory target has no parent",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("directory");
    let suffix = uuid::Uuid::new_v4().simple();
    let temporary = parent.join(format!(".{name}.tmp.{suffix}"));
    let previous = parent.join(format!(".{name}.previous.{suffix}"));
    copy_directory_tree(source, &temporary)?;

    let target_exists = match std::fs::symlink_metadata(target) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            let _ = remove_path(&temporary);
            return Err(error);
        }
    };
    if target_exists {
        if let Err(error) = std::fs::rename(target, &previous) {
            let _ = remove_path(&temporary);
            return Err(error);
        }
    }
    if let Err(error) = std::fs::rename(&temporary, target) {
        if target_exists {
            let _ = std::fs::rename(&previous, target);
        }
        let _ = remove_path(&temporary);
        return Err(error);
    }
    if target_exists {
        if let Err(error) = cleanup_previous(&previous) {
            tracing::warn!(
                path = %previous.display(),
                %error,
                "Failed to remove a previous directory after committing its replacement"
            );
        }
    }
    Ok(())
}

fn digest_directory_entries(
    root: &Path,
    relative: &Path,
    encoded: &mut Vec<u8>,
) -> Result<(), std::io::Error> {
    for name in sorted_entry_names(&root.join(relative))? {
        let child_relative = relative.join(&name);
        let child = root.join(&child_relative);
        let metadata = std::fs::symlink_metadata(&child)?;
        let path = child_relative.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Directory contains a non-UTF-8 path: {}", child.display()),
            )
        })?;
        if metadata.file_type().is_symlink() {
            let link = std::fs::read_link(&child)?;
            validate_contained_link(&child_relative, &link)?;
            append_digest_record(
                encoded,
                b'L',
                path.as_bytes(),
                0,
                link.as_os_str().as_encoded_bytes(),
            );
        } else if metadata.is_dir() {
            append_digest_record(
                encoded,
                b'D',
                path.as_bytes(),
                metadata.permissions().mode(),
                &[],
            );
            digest_directory_entries(root, &child_relative, encoded)?;
        } else if metadata.is_file() {
            let bytes = std::fs::read(&child)?;
            append_digest_record(
                encoded,
                b'F',
                path.as_bytes(),
                metadata.permissions().mode(),
                &bytes,
            );
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unsupported directory entry: {}", child.display()),
            ));
        }
    }
    Ok(())
}

fn copy_directory_entries(
    source_root: &Path,
    target_root: &Path,
    relative: &Path,
) -> Result<(), std::io::Error> {
    for name in sorted_entry_names(&source_root.join(relative))? {
        let child_relative = relative.join(&name);
        let source = source_root.join(&child_relative);
        let target = target_root.join(&child_relative);
        let metadata = std::fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() {
            let link = std::fs::read_link(&source)?;
            validate_contained_link(&child_relative, &link)?;
            std::os::unix::fs::symlink(link, target)?;
        } else if metadata.is_dir() {
            std::fs::create_dir(&target)?;
            copy_directory_entries(source_root, target_root, &child_relative)?;
            std::fs::set_permissions(
                &target,
                std::fs::Permissions::from_mode(metadata.permissions().mode()),
            )?;
        } else if metadata.is_file() {
            std::fs::copy(&source, &target)?;
            std::fs::set_permissions(
                &target,
                std::fs::Permissions::from_mode(metadata.permissions().mode()),
            )?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unsupported directory entry: {}", source.display()),
            ));
        }
    }
    Ok(())
}

fn sorted_entry_names(directory: &Path) -> Result<Vec<OsString>, std::io::Error> {
    let mut names = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort_by(|left, right| left.as_encoded_bytes().cmp(right.as_encoded_bytes()));
    Ok(names)
}

fn validate_contained_link(relative: &Path, target: &Path) -> Result<(), std::io::Error> {
    if target.is_absolute() {
        return Err(unsafe_link_error(relative, target));
    }
    let mut depth = relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir => return Err(unsafe_link_error(relative, target)),
            Component::RootDir | Component::Prefix(_) => {
                return Err(unsafe_link_error(relative, target))
            }
        }
    }
    Ok(())
}

fn unsafe_link_error(relative: &Path, target: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!(
            "Directory symlink escapes its source tree: {} -> {}",
            relative.display(),
            target.display()
        ),
    )
}

fn append_digest_record(encoded: &mut Vec<u8>, kind: u8, path: &[u8], mode: u32, body: &[u8]) {
    encoded.push(kind);
    encoded.extend_from_slice(&(path.len() as u64).to_be_bytes());
    encoded.extend_from_slice(path);
    encoded.extend_from_slice(&mode.to_be_bytes());
    encoded.extend_from_slice(&(body.len() as u64).to_be_bytes());
    encoded.extend_from_slice(body);
}

fn remove_path(path: &Path) -> Result<(), std::io::Error> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)
        }
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn write_symlink_atomic(target: &Path, source: &Path) -> Result<(), std::io::Error> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Symlink target has no parent",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("link");
    let temporary = parent.join(format!(".{name}.tmp.{}", uuid::Uuid::new_v4().simple()));
    std::os::unix::fs::symlink(source, &temporary)?;
    if let Err(error) = std::fs::rename(&temporary, target) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn content_error(mutation: &PlannedMutation, error: serde_json::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::InvalidPlan,
        message: error.to_string(),
        agent_id: None,
        installation_id: Some(mutation.resource.installation_id.clone()),
        resource: Some(mutation.resource.clone()),
        retryable: false,
        details: None,
    }
}

fn standalone_io_error(path: &Path, error: std::io::Error) -> AgentError {
    AgentError {
        code: AgentErrorCode::Io,
        message: format!("Failed to access {}: {error}", path.display()),
        agent_id: None,
        installation_id: None,
        resource: None,
        retryable: true,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_digest_is_stable_and_tracks_content_and_mode() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        for root in [left.path(), right.path()] {
            std::fs::create_dir(root.join("nested")).unwrap();
            std::fs::write(root.join("nested/plugin.json"), b"{}\n").unwrap();
            std::fs::write(root.join("README.md"), b"demo\n").unwrap();
        }

        let initial = directory_tree_digest(left.path()).unwrap();
        assert_eq!(initial, directory_tree_digest(right.path()).unwrap());

        std::fs::write(right.path().join("README.md"), b"changed\n").unwrap();
        assert_ne!(initial, directory_tree_digest(right.path()).unwrap());

        std::fs::write(right.path().join("README.md"), b"demo\n").unwrap();
        let mode = std::fs::metadata(right.path().join("README.md"))
            .unwrap()
            .permissions()
            .mode();
        std::fs::set_permissions(
            right.path().join("README.md"),
            std::fs::Permissions::from_mode(mode ^ 0o100),
        )
        .unwrap();
        assert_ne!(initial, directory_tree_digest(right.path()).unwrap());
    }

    #[test]
    fn directory_copy_preserves_contained_symlinks_and_rejects_escapes() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::write(source.join("nested/file.txt"), "safe").unwrap();
        std::os::unix::fs::symlink("nested/file.txt", source.join("safe-link")).unwrap();
        let chmod_status = std::process::Command::new("chmod")
            .args(["-h", "777"])
            .arg(source.join("safe-link"))
            .status()
            .unwrap();
        assert!(chmod_status.success());

        copy_directory_tree(&source, &target).unwrap();
        assert_eq!(
            std::fs::read_link(target.join("safe-link")).unwrap(),
            PathBuf::from("nested/file.txt")
        );
        assert_eq!(
            directory_tree_digest(&source).unwrap(),
            directory_tree_digest(&target).unwrap()
        );

        std::os::unix::fs::symlink("../../outside", source.join("nested/escape")).unwrap();
        let error = directory_tree_digest(&source).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn directory_copy_applies_read_only_modes_after_copying_contents() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let source_nested = source.join("nested");
        let target_nested = target.join("nested");
        std::fs::create_dir_all(&source_nested).unwrap();
        std::fs::write(source_nested.join("plugin.json"), "{}\n").unwrap();
        std::fs::set_permissions(&source_nested, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o555)).unwrap();

        copy_directory_tree(&source, &target).unwrap();

        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_eq!(
            std::fs::metadata(&target_nested)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o555
        );
        assert_eq!(
            directory_tree_digest(&source).unwrap(),
            directory_tree_digest(&target).unwrap()
        );

        for directory in [&source, &source_nested, &target, &target_nested] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn atomic_directory_write_replaces_the_previous_tree() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(source.join("new.txt"), "new").unwrap();
        std::fs::write(target.join("old.txt"), "old").unwrap();

        write_directory_atomic(&target, &source).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("new.txt")).unwrap(),
            "new"
        );
        assert!(!target.join("old.txt").exists());
        assert_eq!(
            directory_tree_digest(&source).unwrap(),
            directory_tree_digest(&target).unwrap()
        );
    }

    #[test]
    fn atomic_directory_cleanup_failure_is_post_commit() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(source.join("new.txt"), "new").unwrap();
        std::fs::write(target.join("old.txt"), "old").unwrap();

        write_directory_atomic_with_cleanup(&target, &source, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected cleanup failure",
            ))
        })
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("new.txt")).unwrap(),
            "new"
        );
        assert!(!target.join("old.txt").exists());
    }
}
