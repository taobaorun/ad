use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fchmod, fstat, fsync, linkat, mkdirat, open, openat, renameat, unlinkat, AtFlags, Dir,
    FileType, Mode, OFlags,
};

use crate::fs::paths::ad_home;

use super::ContentDigest;

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

#[derive(Debug)]
pub(super) struct ExecutionState {
    locks: StateDirectory,
    journals: StateDirectory,
    ownership: StateDirectory,
    backups: StateDirectory,
    history: StateDirectory,
}

impl ExecutionState {
    pub(super) fn open() -> std::io::Result<Self> {
        let root = ad_home().map_err(|error| std::io::Error::other(error.to_string()))?;
        Self::open_at(&root)
    }

    pub(super) fn open_at(root: &Path) -> std::io::Result<Self> {
        let parent_path = root
            .parent()
            .ok_or_else(|| invalid_input("AD state root has no parent"))?;
        let root_name = root
            .file_name()
            .ok_or_else(|| invalid_input("AD state root has no name"))?;
        validate_name(root_name)?;
        let parent = open(parent_path, DIRECTORY_FLAGS, Mode::empty())?;
        let root = open_or_create_directory(&parent, root_name, root)?;
        let state = root.open_or_create_directory("state")?;
        let locks = state.open_or_create_directory("execution-locks")?;
        let journals = state.open_or_create_directory("operation-journals")?;
        let ownership = state.open_or_create_directory("resource-ownership")?;
        let backups = root
            .open_or_create_directory("backups")?
            .open_or_create_directory("operations")?;
        let history = root
            .open_or_create_directory("history")?
            .open_or_create_directory("operations")?;
        Ok(Self {
            locks,
            journals,
            ownership,
            backups,
            history,
        })
    }

    pub(super) fn locks(&self) -> &StateDirectory {
        &self.locks
    }

    pub(super) fn journals(&self) -> &StateDirectory {
        &self.journals
    }

    pub(super) fn ownership(&self) -> &StateDirectory {
        &self.ownership
    }

    pub(super) fn backups(&self) -> &StateDirectory {
        &self.backups
    }

    pub(super) fn history(&self) -> &StateDirectory {
        &self.history
    }
}

#[derive(Debug)]
pub(super) struct StateDirectory {
    fd: OwnedFd,
    display_path: PathBuf,
}

impl StateDirectory {
    pub(super) fn duplicate(&self) -> std::io::Result<Self> {
        Ok(Self {
            fd: rustix::io::dup(&self.fd)?,
            display_path: self.display_path.clone(),
        })
    }

    pub(super) fn display_path(&self) -> &Path {
        &self.display_path
    }

    pub(super) fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    pub(super) fn write_atomic(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.write_atomic_with(name, bytes, true, |_| Ok(()))
    }

    pub(super) fn write_atomic_new(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.write_atomic_with(name, bytes, false, |_| Ok(()))
    }

    pub(super) fn write_atomic_with(
        &self,
        name: &str,
        bytes: &[u8],
        replace: bool,
        mut boundary: impl FnMut(StateWriteBoundary) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        validate_name(OsStr::new(name))?;
        let temporary = format!(".{name}.tmp.{}", uuid::Uuid::new_v4().simple());
        let result = (|| {
            let fd = openat(
                &self.fd,
                temporary.as_str(),
                OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            )?;
            let mut file = File::from(fd);
            file.write_all(bytes)?;
            boundary(StateWriteBoundary::FileSync)?;
            file.sync_all()?;
            boundary(StateWriteBoundary::Publish)?;
            if replace {
                renameat(&self.fd, temporary.as_str(), &self.fd, name)?;
            } else {
                linkat(
                    &self.fd,
                    temporary.as_str(),
                    &self.fd,
                    name,
                    AtFlags::empty(),
                )?;
                unlinkat(&self.fd, temporary.as_str(), AtFlags::empty())?;
            }
            boundary(StateWriteBoundary::ParentSync)?;
            fsync(&self.fd).map_err(Into::into)
        })();
        if result.is_err() {
            let _ = unlinkat(&self.fd, temporary.as_str(), AtFlags::empty());
        }
        result
    }

    pub(super) fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        validate_name(OsStr::new(name))?;
        let fd = openat(
            &self.fd,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        let mut bytes = Vec::new();
        File::from(fd).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub(super) fn sync(&self) -> std::io::Result<()> {
        fsync(&self.fd).map_err(Into::into)
    }

    pub(super) fn modified(&self, name: &str) -> std::io::Result<std::time::SystemTime> {
        validate_name(OsStr::new(name))?;
        let fd = openat(
            &self.fd,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        File::from(fd).metadata()?.modified()
    }

    pub(super) fn directory_digest(&self, name: &str) -> std::io::Result<ContentDigest> {
        validate_name(OsStr::new(name))?;
        super::execution_tree::directory_digest(&self.fd, OsStr::new(name))
    }

    pub(super) fn digest(&self) -> std::io::Result<ContentDigest> {
        super::execution_tree::directory_digest_fd(&self.fd)
    }

    pub(super) fn remove(&self, name: &str) -> std::io::Result<()> {
        validate_name(OsStr::new(name))?;
        super::execution_tree::remove_entry(&self.fd, OsStr::new(name))
    }

    pub(super) fn entry_names(&self) -> std::io::Result<Vec<OsString>> {
        let mut names = Dir::read_from(rustix::io::dup(&self.fd)?)?
            .filter_map(|entry| match entry {
                Ok(entry) if matches!(entry.file_name().to_bytes(), b"." | b"..") => None,
                Ok(entry) => Some(Ok(OsString::from_vec(
                    entry.file_name().to_bytes().to_vec(),
                ))),
                Err(error) => Some(Err(std::io::Error::from(error))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        Ok(names)
    }

    pub(super) fn create_directory(&self, name: &str) -> std::io::Result<Self> {
        validate_name(OsStr::new(name))?;
        mkdirat(&self.fd, name, Mode::RWXU)?;
        fsync(&self.fd)?;
        self.open_directory(name)
    }

    pub(super) fn open_directory(&self, name: &str) -> std::io::Result<Self> {
        validate_name(OsStr::new(name))?;
        let fd = openat(&self.fd, name, DIRECTORY_FLAGS, Mode::empty())?;
        validate_directory(&fd, &self.display_path.join(name))?;
        Ok(Self {
            fd,
            display_path: self.display_path.join(name),
        })
    }

    pub(super) fn open_or_create_directory(&self, name: &str) -> std::io::Result<Self> {
        match self.open_directory(name) {
            Ok(directory) => Ok(directory),
            Err(error) if error.raw_os_error() == Some(rustix::io::Errno::NOENT.raw_os_error()) => {
                self.create_directory(name)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn open_lock(&self, name: &str) -> std::io::Result<File> {
        validate_name(OsStr::new(name))?;
        let fd = openat(
            &self.fd,
            name,
            OFlags::CREATE | OFlags::RDWR | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?;
        let stat = fstat(&fd)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
            || stat.st_uid != rustix::process::geteuid().as_raw()
            || stat.st_nlink != 1
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Execution lock is not a private current-user regular file",
            ));
        }
        fchmod(&fd, Mode::RUSR | Mode::WUSR)?;
        fsync(&self.fd)?;
        Ok(File::from(fd))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StateWriteBoundary {
    FileSync,
    Publish,
    ParentSync,
}

fn open_or_create_directory(
    parent: &OwnedFd,
    name: &OsStr,
    display_path: &Path,
) -> std::io::Result<StateDirectory> {
    let fd = match openat(parent, name, DIRECTORY_FLAGS, Mode::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => {
            mkdirat(parent, name, Mode::RWXU)?;
            fsync(parent)?;
            openat(parent, name, DIRECTORY_FLAGS, Mode::empty())?
        }
        Err(error) => return Err(error.into()),
    };
    validate_directory(&fd, display_path)?;
    Ok(StateDirectory {
        fd,
        display_path: display_path.to_path_buf(),
    })
}

fn validate_directory(fd: &OwnedFd, path: &Path) -> std::io::Result<()> {
    let stat = fstat(fd)?;
    let mode = stat.st_mode & 0o777;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || mode & 0o022 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("Unsafe AD state directory: {}", path.display()),
        ));
    }
    Ok(())
}

fn validate_name(name: &OsStr) -> std::io::Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(invalid_input("AD state name must be one path component"));
    }
    Ok(())
}

fn invalid_input(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}
