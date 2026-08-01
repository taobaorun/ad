use std::fs::File;
use std::path::Path;

use rustix::fs::{flock, open, openat, FlockOperation, Mode, OFlags};

use super::SkillArtifactError;

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

pub(super) fn acquire_staging_lease(operation_root: &Path) -> Result<File, SkillArtifactError> {
    let root = open(operation_root, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|source| io_error(operation_root, source.into()))?;
    let lease = openat(
        &root,
        ".lease",
        OFlags::CREATE | OFlags::EXCL | OFlags::RDWR | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(|source| io_error(&operation_root.join(".lease"), source.into()))?;
    flock(&lease, FlockOperation::NonBlockingLockExclusive)
        .map_err(|source| io_error(&operation_root.join(".lease"), source.into()))?;
    Ok(lease)
}

pub(super) fn acquire_cleanup_lease(
    operation_root: &Path,
) -> Result<Option<File>, SkillArtifactError> {
    let root = open(operation_root, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|source| io_error(operation_root, source.into()))?;
    let lease = match openat(
        &root,
        ".lease",
        OFlags::RDWR | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(lease) => File::from(lease),
        Err(rustix::io::Errno::NOENT) => return Ok(Some(File::from(root))),
        Err(source) => return Err(io_error(&operation_root.join(".lease"), source.into())),
    };
    match flock(&lease, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(lease)),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
        Err(source) => Err(io_error(&operation_root.join(".lease"), source.into())),
    }
}

fn io_error(path: &Path, source: std::io::Error) -> SkillArtifactError {
    SkillArtifactError::Io {
        path: path.display().to_string(),
        source,
    }
}
