use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fchmod, fstat, fsync, mkdirat, openat, readlinkat, renameat, statat, symlinkat, unlinkat,
    AtFlags, Dir, FileType, Mode, OFlags,
};

use super::ContentDigest;

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

pub(super) fn directory_digest(parent: &OwnedFd, name: &OsStr) -> std::io::Result<ContentDigest> {
    let root = openat(parent, name, DIRECTORY_FLAGS, Mode::empty())?;
    directory_digest_fd(&root)
}

pub(super) fn directory_digest_fd(root: &OwnedFd) -> std::io::Result<ContentDigest> {
    let mut encoded = Vec::new();
    digest_entries(root, Path::new(""), &mut encoded)?;
    Ok(ContentDigest::sha256(&encoded))
}

pub(super) fn write_directory_atomic(
    parent: &OwnedFd,
    name: &OsStr,
    source: &Path,
) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_data("Directory source is not a physical directory"));
    }
    let temporary = sibling_name(name, "tmp");
    mkdirat(parent, temporary.as_os_str(), Mode::RWXU)?;
    let temporary_fd = openat(
        parent,
        temporary.as_os_str(),
        DIRECTORY_FLAGS,
        Mode::empty(),
    )?;
    let populated = copy_path_entries(source, Path::new(""), &temporary_fd)
        .and_then(|()| {
            fchmod(
                &temporary_fd,
                Mode::from_raw_mode(metadata.permissions().mode() as _),
            )
            .map_err(std::io::Error::from)
        })
        .and_then(|()| fsync(&temporary_fd).map_err(Into::into));
    drop(temporary_fd);
    if let Err(error) = populated {
        let _ = remove_entry(parent, temporary.as_os_str());
        return Err(error);
    }
    publish_directory(parent, name, temporary.as_os_str())
}

pub(super) fn write_directory_atomic_from(
    parent: &OwnedFd,
    name: &OsStr,
    source: &OwnedFd,
) -> std::io::Result<()> {
    let metadata = fstat(source)?;
    let temporary = sibling_name(name, "tmp");
    mkdirat(parent, temporary.as_os_str(), Mode::RWXU)?;
    let temporary_fd = openat(
        parent,
        temporary.as_os_str(),
        DIRECTORY_FLAGS,
        Mode::empty(),
    )?;
    let populated = copy_fd_entries(source, &temporary_fd)
        .and_then(|()| {
            fchmod(&temporary_fd, Mode::from_raw_mode(metadata.st_mode as _))
                .map_err(std::io::Error::from)
        })
        .and_then(|()| fsync(&temporary_fd).map_err(Into::into));
    drop(temporary_fd);
    if let Err(error) = populated {
        let _ = remove_entry(parent, temporary.as_os_str());
        return Err(error);
    }
    publish_directory(parent, name, temporary.as_os_str())
}

fn publish_directory(parent: &OwnedFd, name: &OsStr, temporary: &OsStr) -> std::io::Result<()> {
    let previous = sibling_name(name, "previous");
    let existed = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => true,
        Err(rustix::io::Errno::NOENT) => false,
        Err(error) => {
            let _ = remove_entry(parent, temporary);
            return Err(error.into());
        }
    };
    if existed {
        if let Err(error) = renameat(parent, name, parent, previous.as_os_str()) {
            let _ = remove_entry(parent, temporary);
            return Err(error.into());
        }
    }
    if let Err(error) = renameat(parent, temporary, parent, name) {
        if existed {
            let _ = renameat(parent, previous.as_os_str(), parent, name);
        }
        let _ = remove_entry(parent, temporary);
        return Err(error.into());
    }
    fsync(parent)?;
    if existed {
        if let Err(error) = remove_entry(parent, previous.as_os_str()) {
            tracing::warn!(%error, "Failed to remove a previous confined directory");
        }
    }
    Ok(())
}

pub(super) fn copy_directory_to(
    parent: &OwnedFd,
    name: &OsStr,
    destination_parent: &OwnedFd,
    destination_name: &OsStr,
) -> std::io::Result<()> {
    let root = openat(parent, name, DIRECTORY_FLAGS, Mode::empty())?;
    let metadata = fstat(&root)?;
    mkdirat(destination_parent, destination_name, Mode::RWXU)?;
    let destination = openat(
        destination_parent,
        destination_name,
        DIRECTORY_FLAGS,
        Mode::empty(),
    )?;
    if let Err(error) = copy_fd_entries(&root, &destination) {
        let _ = remove_entry(destination_parent, destination_name);
        return Err(error);
    }
    fchmod(&destination, Mode::from_raw_mode(metadata.st_mode as _))?;
    fsync(&destination)?;
    fsync(destination_parent).map_err(Into::into)
}

pub(super) fn remove_entry(parent: &OwnedFd, name: &OsStr) -> std::io::Result<()> {
    let stat = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
        let directory = openat(parent, name, DIRECTORY_FLAGS, Mode::empty())?;
        fchmod(&directory, Mode::RWXU)?;
        for child in entry_names(&directory)? {
            remove_entry(&directory, child.as_os_str())?;
        }
        unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    } else {
        unlinkat(parent, name, AtFlags::empty())?;
    }
    fsync(parent).map_err(Into::into)
}

pub(super) fn write_symlink_atomic(
    parent: &OwnedFd,
    name: &OsStr,
    source: &Path,
) -> std::io::Result<()> {
    let temporary = sibling_name(name, "tmp");
    symlinkat(source, parent, temporary.as_os_str())?;
    if let Err(error) = renameat(parent, temporary.as_os_str(), parent, name) {
        let _ = unlinkat(parent, temporary.as_os_str(), AtFlags::empty());
        return Err(error.into());
    }
    fsync(parent).map_err(Into::into)
}

fn digest_entries(
    directory: &OwnedFd,
    relative: &Path,
    encoded: &mut Vec<u8>,
) -> std::io::Result<()> {
    for name in entry_names(directory)? {
        let child_relative = relative.join(&name);
        let path = child_relative
            .to_str()
            .ok_or_else(|| invalid_data("Directory contains a non-UTF-8 path"))?;
        let stat = statat(directory, name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Symlink => {
                let link = link_target(directory, name.as_os_str())?;
                validate_contained_link(&child_relative, Path::new(&link))?;
                append_record(
                    encoded,
                    b'L',
                    path.as_bytes(),
                    0,
                    link.as_os_str().as_bytes(),
                );
            }
            FileType::Directory => {
                append_record(encoded, b'D', path.as_bytes(), stat.st_mode as u32, &[]);
                let child = openat(directory, name.as_os_str(), DIRECTORY_FLAGS, Mode::empty())?;
                digest_entries(&child, &child_relative, encoded)?;
            }
            FileType::RegularFile => {
                let bytes = read_file(directory, name.as_os_str())?;
                append_record(encoded, b'F', path.as_bytes(), stat.st_mode as u32, &bytes);
            }
            _ => return Err(invalid_data("Unsupported directory entry")),
        }
    }
    Ok(())
}

fn copy_path_entries(source: &Path, relative: &Path, destination: &OwnedFd) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(source)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    for name in entries {
        let child_relative = relative.join(&name);
        let child = source.join(&name);
        let metadata = std::fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() {
            let link = std::fs::read_link(&child)?;
            validate_contained_link(&child_relative, &link)?;
            symlinkat(&link, destination, name.as_os_str())?;
        } else if metadata.is_dir() {
            mkdirat(destination, name.as_os_str(), Mode::RWXU)?;
            let child_fd = openat(
                destination,
                name.as_os_str(),
                DIRECTORY_FLAGS,
                Mode::empty(),
            )?;
            copy_path_entries(&child, &child_relative, &child_fd)?;
            fchmod(
                &child_fd,
                Mode::from_raw_mode(metadata.permissions().mode() as _),
            )?;
            fsync(&child_fd)?;
        } else if metadata.is_file() {
            write_file(
                destination,
                name.as_os_str(),
                &std::fs::read(&child)?,
                metadata.permissions().mode(),
            )?;
        } else {
            return Err(invalid_data("Unsupported directory source entry"));
        }
    }
    fsync(destination).map_err(Into::into)
}

fn copy_fd_entries(source: &OwnedFd, destination: &OwnedFd) -> std::io::Result<()> {
    for name in entry_names(source)? {
        let stat = statat(source, name.as_os_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Symlink => {
                symlinkat(
                    link_target(source, name.as_os_str())?,
                    destination,
                    name.as_os_str(),
                )?;
            }
            FileType::Directory => {
                mkdirat(destination, name.as_os_str(), Mode::RWXU)?;
                let source_child =
                    openat(source, name.as_os_str(), DIRECTORY_FLAGS, Mode::empty())?;
                let destination_child = openat(
                    destination,
                    name.as_os_str(),
                    DIRECTORY_FLAGS,
                    Mode::empty(),
                )?;
                copy_fd_entries(&source_child, &destination_child)?;
                fchmod(&destination_child, Mode::from_raw_mode(stat.st_mode as _))?;
                fsync(&destination_child)?;
            }
            FileType::RegularFile => {
                write_file(
                    destination,
                    name.as_os_str(),
                    &read_file(source, name.as_os_str())?,
                    stat.st_mode as u32,
                )?;
            }
            _ => return Err(invalid_data("Unsupported directory entry")),
        }
    }
    fsync(destination).map_err(Into::into)
}

fn entry_names(directory: &OwnedFd) -> std::io::Result<Vec<OsString>> {
    let mut names = Dir::read_from(rustix::io::dup(directory)?)?
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_name().to_bytes() == b"." => None,
            Ok(entry) if entry.file_name().to_bytes() == b".." => None,
            Ok(entry) => Some(Ok(OsString::from_vec(
                entry.file_name().to_bytes().to_vec(),
            ))),
            Err(error) => Some(Err(std::io::Error::from(error))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(names)
}

fn read_file(parent: &OwnedFd, name: &OsStr) -> std::io::Result<Vec<u8>> {
    let fd = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut bytes = Vec::new();
    File::from(fd).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_file(parent: &OwnedFd, name: &OsStr, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let fd = openat(
        parent,
        name,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
        Mode::from_raw_mode(mode as _),
    )?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()
}

fn link_target(parent: &OwnedFd, name: &OsStr) -> std::io::Result<OsString> {
    Ok(OsString::from_vec(
        readlinkat(parent, name, Vec::new())?.as_bytes().to_vec(),
    ))
}

fn validate_contained_link(relative: &Path, target: &Path) -> std::io::Result<()> {
    if target.is_absolute() {
        return Err(invalid_data("Directory symlink escapes its source tree"));
    }
    let mut depth = relative
        .parent()
        .map_or(0, |path| path.components().count());
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(invalid_data("Directory symlink escapes its source tree")),
        }
    }
    Ok(())
}

fn append_record(encoded: &mut Vec<u8>, kind: u8, path: &[u8], mode: u32, body: &[u8]) {
    encoded.push(kind);
    encoded.extend_from_slice(&(path.len() as u64).to_be_bytes());
    encoded.extend_from_slice(path);
    encoded.extend_from_slice(&mode.to_be_bytes());
    encoded.extend_from_slice(&(body.len() as u64).to_be_bytes());
    encoded.extend_from_slice(body);
}

fn sibling_name(name: &OsStr, kind: &str) -> OsString {
    let mut sibling = OsString::from(".");
    sibling.push(name);
    sibling.push(format!(".{kind}.{}", uuid::Uuid::new_v4().simple()));
    sibling
}

fn invalid_data(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
