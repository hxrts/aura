//! Descriptor-relative native profile IO. Directory handles remain alive through
//! data publication and durability acknowledgment; no descendant is re-resolved
//! through a process working directory or a symlink-following pathname.
use rustix::fs::{linkat, mkdirat, openat, renameat, unlinkat, AtFlags, Mode, OFlags, CWD};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct ProfileDirectory {
    file: Arc<File>,
}
impl ProfileDirectory {
    pub(crate) fn open(path: &Path) -> std::io::Result<Self> {
        let physical = std::fs::canonicalize(path)?;
        Ok(Self {
            file: Arc::new(File::from(openat(
                CWD,
                physical,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?)),
        })
    }
    pub(crate) fn from_file(file: File) -> Self {
        Self {
            file: Arc::new(file),
        }
    }
    pub(crate) fn require_private(&self) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let metadata = self.file.metadata()?;
        if !metadata.is_dir() || metadata.mode() & 0o077 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "secure directory is not private",
            ));
        }
        Ok(())
    }
    pub(crate) fn same_directory(&self, other: &Self) -> std::io::Result<bool> {
        use std::os::unix::fs::MetadataExt;
        let a = self.file.metadata()?;
        let b = other.file.metadata()?;
        Ok(a.dev() == b.dev() && a.ino() == b.ino())
    }
    fn walk(&self, path: &Path, create: bool) -> std::io::Result<Vec<Self>> {
        let mut chain = vec![self.clone()];
        for component in path.components() {
            let Component::Normal(name) = component else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "expected normal profile path component",
                ));
            };
            let parent = chain
                .last()
                .ok_or_else(|| std::io::Error::other("missing profile root"))?;
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let next = match openat(parent.file.as_ref(), name, flags, Mode::empty()) {
                Ok(fd) => fd,
                Err(rustix::io::Errno::NOENT) if create => {
                    match mkdirat(
                        parent.file.as_ref(),
                        name,
                        Mode::RUSR | Mode::WUSR | Mode::XUSR,
                    ) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(e) => return Err(e.into()),
                    }
                    // Parent entry is acknowledged before later data can publish.
                    parent.file.sync_all()?;
                    openat(parent.file.as_ref(), name, flags, Mode::empty())?
                }
                Err(e) => return Err(e.into()),
            };
            chain.push(Self::from_file(File::from(next)));
        }
        Ok(chain)
    }
    pub(crate) fn child(&self, path: &Path, create: bool) -> std::io::Result<Self> {
        self.walk(path, create)?
            .pop()
            .ok_or_else(|| std::io::Error::other("missing directory"))
    }
    fn parent(
        &self,
        path: &Path,
        create: bool,
    ) -> std::io::Result<(Vec<Self>, std::ffi::OsString)> {
        let name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("missing profile filename"))?
            .to_owned();
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        Ok((self.walk(parent, create)?, name))
    }
    pub(crate) fn read(&self, path: &Path, private: bool) -> std::io::Result<Option<Vec<u8>>> {
        let (chain, name) = match self.parent(path, false) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if private {
            for directory in &chain {
                directory.require_private()?;
            }
        }
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing directory"))?;
        let fd = match openat(
            parent.file.as_ref(),
            &name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut file = File::from(fd);
        let metadata = file.metadata()?;
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_file() || metadata.nlink() != 1 || (private && metadata.mode() & 0o077 != 0)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "profile value is not an unaliased private regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }
    pub(crate) fn prepare(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> std::io::Result<PreparedProfileFile> {
        self.prepare_record(path, bytes, false)
    }
    pub(crate) fn prepare_private(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> std::io::Result<PreparedProfileFile> {
        self.prepare_record(path, bytes, true)
    }
    fn prepare_record(
        &self,
        path: &Path,
        bytes: &[u8],
        private: bool,
    ) -> std::io::Result<PreparedProfileFile> {
        let (chain, name) = self.parent(path, true)?;
        if private {
            for directory in &chain {
                directory.require_private()?;
            }
        }
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing directory"))?;
        for _ in 0..16 {
            use rand::RngCore;
            let mut nonce = [0; 16];
            rand::rngs::OsRng
                .try_fill_bytes(&mut nonce)
                .map_err(|source| {
                    std::io::Error::other(aura_core::AuraError::Crypto {
                        message: "profile staging entropy failed".into(),
                        source: Some(Arc::new(source)),
                    })
                })?;
            let staged = format!(".aura-stage-{}", hex::encode(nonce));
            let fd = match openat(
                parent.file.as_ref(),
                &staged,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => fd,
                Err(rustix::io::Errno::EXIST) => continue,
                Err(e) => return Err(e.into()),
            };
            let mut file = File::from(fd);
            file.write_all(bytes)?;
            file.sync_all()?;
            return Ok(PreparedProfileFile {
                chain,
                name,
                staged,
            });
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "profile staging attempts exhausted",
        ))
    }
    pub(crate) fn remove(&self, path: &Path) -> std::io::Result<bool> {
        let (chain, name) = match self.parent(path, false) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing directory"))?;
        match unlinkat(parent.file.as_ref(), name, AtFlags::empty()) {
            Ok(()) => {
                parent.file.sync_all()?;
                Ok(true)
            }
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    pub(crate) fn names(&self) -> std::io::Result<Vec<std::ffi::OsString>> {
        use std::os::unix::ffi::OsStrExt;
        let mut names = Vec::new();
        for entry in rustix::fs::Dir::read_from(self.file.as_ref())? {
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if bytes != b"." && bytes != b".." {
                names.push(std::ffi::OsStr::from_bytes(bytes).to_owned());
            }
        }
        Ok(names)
    }
    pub(crate) fn clear_data(&self) -> std::io::Result<()> {
        for name in self.names()? {
            if name == std::ffi::OsStr::new(".aura-profile-owner.lock") {
                continue;
            }
            match self.child(Path::new(&name), false) {
                Ok(child) => {
                    child.clear_data()?;
                    unlinkat(self.file.as_ref(), &name, AtFlags::REMOVEDIR)?;
                }
                Err(e) if e.raw_os_error() == Some(rustix::io::Errno::NOTDIR.raw_os_error()) => {
                    unlinkat(self.file.as_ref(), &name, AtFlags::empty())?;
                }
                Err(e) => return Err(e),
            }
        }
        self.file.sync_all()
    }
    pub(crate) fn files(&self) -> std::io::Result<Vec<PathBuf>> {
        let mut result = Vec::new();
        let mut stack = vec![(self.clone(), PathBuf::new())];
        while let Some((directory, prefix)) = stack.pop() {
            for name in directory.names()? {
                let path = prefix.join(&name);
                match directory.child(Path::new(&name), false) {
                    Ok(child) => stack.push((child, path)),
                    Err(e)
                        if e.raw_os_error() == Some(rustix::io::Errno::NOTDIR.raw_os_error()) =>
                    {
                        // Open with NOFOLLOW validates the actual entry; symlinks
                        // and special files are rejected rather than traversed.
                        directory.read(Path::new(&name), false)?;
                        result.push(path);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(result)
    }
}
#[derive(Debug)]
pub(crate) struct PreparedProfileFile {
    chain: Vec<ProfileDirectory>,
    name: std::ffi::OsString,
    staged: String,
}
impl PreparedProfileFile {
    pub(crate) fn publish(&self, immutable: bool) -> std::io::Result<bool> {
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing publication directory"))?;
        if immutable {
            match linkat(
                parent.file.as_ref(),
                &self.staged,
                parent.file.as_ref(),
                &self.name,
                AtFlags::empty(),
            ) {
                Ok(()) => {}
                Err(rustix::io::Errno::EXIST) => {
                    unlinkat(parent.file.as_ref(), &self.staged, AtFlags::empty())?;
                    parent.file.sync_all()?;
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            }
            unlinkat(parent.file.as_ref(), &self.staged, AtFlags::empty())?;
        } else {
            renameat(
                parent.file.as_ref(),
                &self.staged,
                parent.file.as_ref(),
                &self.name,
            )?;
        }
        Ok(true)
    }
    pub(crate) fn acknowledge(&self) -> std::io::Result<()> {
        for directory in self.chain.iter().rev() {
            directory.file.sync_all()?;
        }
        Ok(())
    }
}
