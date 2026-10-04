//! Descriptor-relative native profile IO. Directory handles remain alive through
//! data publication and durability acknowledgment; no descendant is re-resolved
//! through a process working directory or a symlink-following pathname.
use rustix::fs::{linkat, mkdirat, openat, renameat, unlinkat, AtFlags, Mode, OFlags, CWD};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StagedPublicationRecoveryError {
    #[error("ambiguous original staged publication")]
    Ambiguous,
    #[error("noncanonical staged publication identity")]
    InvalidIdentity,
    #[error("staged publication conflicts with acknowledged original")]
    ConflictingOriginal,
    #[error("unowned staged publication in selected provider")]
    UnownedStage,
    #[error("selected provider directory depth {observed_depth} exceeds secure layout depth {maximum_depth}")]
    InventoryLimit {
        maximum_depth: usize,
        observed_depth: usize,
    },
    #[error("original staged publication belongs to another selected provider or target")]
    ForeignPublicationOwner,
    #[error("recovery publication is not a private regular file with a valid link count")]
    InvalidPublicationFile,
    #[error("recovery ciphertext length {observed} exceeds {maximum}")]
    PublicationTooLarge { maximum: u64, observed: u64 },
    #[error("recovery publication read budget overflows")]
    ReadBudgetOverflow,
}

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
    pub(crate) fn read_bounded(
        &self,
        path: &Path,
        private: bool,
        maximum_bytes: usize,
    ) -> std::io::Result<Option<Vec<u8>>> {
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
        let file = File::from(fd);
        let metadata = file.metadata()?;
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_file() || metadata.nlink() != 1 || (private && metadata.mode() & 0o077 != 0)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "profile value is not an unaliased private regular file",
            ));
        }
        let maximum_bytes = u64::try_from(maximum_bytes)
            .map_err(|source| std::io::Error::new(std::io::ErrorKind::InvalidInput, source))?;
        if metadata.len() > maximum_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "profile record exceeds bounded reader",
            ));
        }
        let read_limit = maximum_bytes.checked_add(1).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "profile bounded reader overflow",
            )
        })?;
        let mut bytes = Vec::new();

        file.take(read_limit).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > maximum_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "profile record grew beyond bounded reader",
            ));
        }
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
            use std::os::unix::ffi::OsStrExt;
            let target = hex::encode(aura_core::hash::hash(name.as_bytes()));
            let staged = format!(".aura-stage-v2-{target}-{}", hex::encode(nonce));
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
            parent.file.sync_all()?;
            return Ok(PreparedProfileFile {
                chain,
                name,
                staged,
                observed_digest: None,
            });
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "profile staging attempts exhausted",
        ))
    }
    /// Return exact target-bound pre-link ciphertext under retained descriptors.
    /// The caller must authenticate and validate it before acknowledging publication.
    pub(crate) fn read_private_publication(
        &self,
        path: &Path,
        maximum: usize,
    ) -> std::io::Result<Option<(Vec<u8>, Option<PreparedProfileFile>)>> {
        use std::os::unix::ffi::OsStrExt;
        let (chain, name) = match self.parent(path, false) {
            Ok(parent) => parent,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(source),
        };
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing staged parent"))?;
        let prefix = format!(
            ".aura-stage-v2-{}-",
            hex::encode(aura_core::hash::hash(name.as_bytes()))
        );
        let Some(staged) = parent.find_original_stage(&prefix)? else {
            return match read_recovery_ciphertext(parent, &name, maximum) {
                Ok((target, bytes)) => {
                    use std::os::unix::fs::MetadataExt;
                    if target.metadata()?.nlink() != 1
                        && !matches_initial_custody(parent, &name, &target, &bytes, maximum)?
                    {
                        return Err(recovery_publication_conflict());
                    }
                    Ok(Some((bytes, None)))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            };
        };
        let staged = staged.into_string().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                StagedPublicationRecoveryError::InvalidIdentity,
            )
        })?;
        let suffix = staged
            .strip_prefix(&prefix)
            .ok_or_else(|| std::io::Error::other("stage binding"))?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                StagedPublicationRecoveryError::InvalidIdentity,
            ));
        }
        use std::os::unix::fs::MetadataExt;
        let (source, bytes) =
            read_recovery_ciphertext(parent, std::ffi::OsStr::new(&staged), maximum)?;
        match read_recovery_ciphertext(parent, &name, maximum) {
            Ok((target, original)) => {
                let source_identity = source.metadata()?;
                let target_identity = target.metadata()?;
                let same_inode = source_identity.dev() == target_identity.dev()
                    && source_identity.ino() == target_identity.ino();
                if original != bytes
                    || ((source_identity.nlink() == 2 || target_identity.nlink() == 2)
                        && !same_inode)
                {
                    return Err(recovery_publication_conflict());
                }
            }
            Err(source_error) if source_error.kind() == std::io::ErrorKind::NotFound => {
                if source.metadata()?.nlink() != 1 {
                    return Err(recovery_publication_conflict());
                }
            }
            Err(source) => return Err(source),
        }
        let observed_digest = Some(aura_core::hash::hash(&bytes));
        Ok(Some((
            bytes,
            Some(PreparedProfileFile {
                chain,
                name,
                staged,
                observed_digest,
            }),
        )))
    }

    /// Observing a differing successor grants no permission to replace a target.
    pub(crate) fn observe_original_successor(
        &self,
        path: &Path,
        maximum: usize,
    ) -> std::io::Result<Option<ObservedOriginalSuccessor>> {
        use std::os::unix::ffi::OsStrExt;
        let (chain, name) = match self.parent(path, false) {
            Ok(parent) => parent,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(source),
        };
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing successor parent"))?;
        let prefix = format!(
            ".aura-stage-v2-{}-",
            hex::encode(aura_core::hash::hash(name.as_bytes()))
        );
        let Some(stage_name) = parent.find_original_stage(&prefix)? else {
            return Ok(None);
        };
        let staged = stage_name.into_string().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                StagedPublicationRecoveryError::InvalidIdentity,
            )
        })?;
        let suffix = staged
            .strip_prefix(&prefix)
            .ok_or_else(recovery_publication_conflict)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                StagedPublicationRecoveryError::InvalidIdentity,
            ));
        }
        let Some(original) = self.read_canonical_private_publication(path, maximum)? else {
            return Ok(None);
        };
        let (_, next) = read_recovery_ciphertext(parent, std::ffi::OsStr::new(&staged), maximum)?;
        if original == next {
            return Ok(None);
        }
        let observed_digest = Some(aura_core::hash::hash(&next));
        Ok(Some(ObservedOriginalSuccessor {
            original,
            next,
            publication: PreparedProfileFile {
                chain,
                name,
                staged,
                observed_digest,
            },
        }))
    }

    pub(crate) fn observe_retained_initial_successor(
        &self,
        path: &Path,
        original: Vec<u8>,
        next: Vec<u8>,
        staged: String,
    ) -> std::io::Result<ObservedOriginalSuccessor> {
        use std::os::unix::ffi::OsStrExt;
        let (chain, name) = self.parent(path, false)?;
        let prefix = format!(
            ".aura-stage-v2-{}-",
            hex::encode(aura_core::hash::hash(name.as_bytes()))
        );
        let suffix = staged
            .strip_prefix(&prefix)
            .ok_or_else(recovery_publication_conflict)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(recovery_publication_conflict());
        }
        Ok(ObservedOriginalSuccessor {
            original,
            next: next.clone(),
            publication: PreparedProfileFile {
                chain,
                name,
                staged,
                observed_digest: Some(aura_core::hash::hash(&next)),
            },
        })
    }

    /// Canonical presence remains distinct from an observed pre-link stage.
    /// Only an exact source/target publication pair permits the temporary second link.
    pub(crate) fn read_canonical_private_publication(
        &self,
        path: &Path,
        maximum: usize,
    ) -> std::io::Result<Option<Vec<u8>>> {
        use std::os::unix::fs::MetadataExt;
        let (chain, name) = match self.parent(path, false) {
            Ok(parent) => parent,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(source),
        };
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing canonical publication parent"))?;
        let (target, bytes) = match read_recovery_ciphertext(parent, &name, maximum) {
            Ok(record) => record,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(source),
        };
        if target.metadata()?.nlink() == 1 {
            return Ok(Some(bytes));
        }
        if matches_initial_custody(parent, &name, &target, &bytes, maximum)? {
            return Ok(Some(bytes));
        }
        let Some((observed, stage)) = self.read_private_publication(path, maximum)? else {
            return Err(recovery_publication_conflict());
        };
        if stage.is_none() || observed != bytes {
            return Err(recovery_publication_conflict());
        }
        Ok(Some(bytes))
    }

    fn find_original_stage(&self, prefix: &str) -> std::io::Result<Option<std::ffi::OsString>> {
        use std::os::unix::ffi::OsStrExt;
        let mut staged = None;
        for entry in rustix::fs::Dir::read_from(self.file.as_ref())? {
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if !bytes.starts_with(prefix.as_bytes()) {
                continue;
            }
            if staged.is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    StagedPublicationRecoveryError::Ambiguous,
                ));
            }
            staged = Some(std::ffi::OsStr::from_bytes(bytes).to_owned());
        }
        Ok(staged)
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
    /// Filename eligibility is observation only; callers authenticate every candidate.
    /// Ordinary leaves stream without imposing a new provider-wide record limit.
    pub(crate) fn require_stage_inventory(&self, allowed: &[&Path]) -> std::io::Result<()> {
        let mut seen = Vec::with_capacity(allowed.len());
        self.require_stage_inventory_at(Path::new(""), 0, allowed, &mut seen)
    }
    fn require_stage_inventory_at(
        &self,
        prefix: &Path,
        depth: usize,
        allowed: &[&Path],
        seen: &mut Vec<PathBuf>,
    ) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        self.require_private()?;
        for entry in rustix::fs::Dir::read_from(self.file.as_ref())? {
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = std::ffi::OsStr::from_bytes(bytes);
            if bytes.starts_with(b".aura-initial-")
                && (prefix.as_os_str().is_empty()
                    || prefix == Path::new(".aura-allocation-lifetime-owner-v1"))
            {
                match self.child(Path::new(name), false) {
                    Ok(_) => {} // Ordinary namespace directories retain their existing meaning.
                    Err(error)
                        if error.raw_os_error()
                            == Some(rustix::io::Errno::NOTDIR.raw_os_error()) =>
                    {
                        if !initial_cutover_entry_matches(prefix, name) {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                StagedPublicationRecoveryError::UnownedStage,
                            ));
                        }
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            if bytes.starts_with(b".aura-stage-") {
                let Some(target) = allowed
                    .iter()
                    .find(|target| stage_matches_target(prefix, name, target))
                else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        StagedPublicationRecoveryError::UnownedStage,
                    ));
                };
                if seen.iter().any(|previous| previous == *target) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        StagedPublicationRecoveryError::Ambiguous,
                    ));
                }
                seen.push(target.to_path_buf());
                continue;
            }
            match self.child(Path::new(name), false) {
                Ok(child) => {
                    if depth == MAX_SECURE_RECORD_DIRECTORY_DEPTH {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            StagedPublicationRecoveryError::InventoryLimit {
                                maximum_depth: MAX_SECURE_RECORD_DIRECTORY_DEPTH,
                                observed_depth: depth + 1,
                            },
                        ));
                    }
                    child.require_stage_inventory_at(
                        &prefix.join(name),
                        depth + 1,
                        allowed,
                        seen,
                    )?;
                }
                Err(source)
                    if source.raw_os_error() == Some(rustix::io::Errno::NOTDIR.raw_os_error()) => {}
                Err(source) => return Err(source),
            }
        }
        Ok(())
    }
    /// Observational inode checks become relevant only after the domain owner
    /// authenticates the journal from which these exact expectations came.
    pub(crate) fn require_archived_initial_custody(
        &self,
        path: &Path,
        staged: &str,
        original: &[u8],
        next: &[u8],
        identities: crate::secure::InitialCutoverIdentities,
        maximum: usize,
    ) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let (chain, _) = self.parent(path, false)?;
        let parent = chain.last().ok_or_else(recovery_publication_conflict)?;
        for (kind, expected, identity) in [
            ("before", original, identities.original),
            ("next", next, identities.next),
            ("displaced", original, identities.original),
        ] {
            let name = format!(".aura-initial-{kind}-{staged}");
            let (file, bytes) =
                read_initial_cutover_ciphertext(parent, std::ffi::OsStr::new(&name), maximum)?;
            let metadata = file.metadata()?;
            if (metadata.dev(), metadata.ino()) != identity || bytes != expected {
                return Err(recovery_publication_conflict());
            }
        }
        Ok(())
    }

    pub(crate) fn require_initial_custody_origins(
        &self,
        path: &Path,
        authenticated_stages: &[String],
    ) -> std::io::Result<()> {
        let (chain, _) = match self.parent(path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let parent = chain.last().ok_or_else(recovery_publication_conflict)?;
        for entry in rustix::fs::Dir::read_from(parent.file.as_ref())? {
            let entry = entry?;
            let name = entry.file_name();
            for kind in ["before", "next", "displaced"] {
                let prefix = format!(".aura-initial-{kind}-");
                if let Some(stage) = name.to_bytes().strip_prefix(prefix.as_bytes()) {
                    if !authenticated_stages
                        .iter()
                        .any(|expected| expected.as_bytes() == stage)
                    {
                        return Err(recovery_publication_conflict());
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn visit_initial_cutover_history(
        &self,
        path: &Path,
        maximum: usize,
        mut visit: impl FnMut(Vec<u8>, String) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let (chain, name) = match self.parent(path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("cutover history parent"))?;
        let prefix = format!(
            ".aura-initial-history-.aura-stage-v2-{}-",
            hex::encode(aura_core::hash::hash(name.as_bytes()))
        );
        let mut count = 0;
        for entry in rustix::fs::Dir::read_from(parent.file.as_ref())? {
            let entry = entry?;
            let candidate = entry.file_name();
            let Some(suffix) = candidate.to_bytes().strip_prefix(prefix.as_bytes()) else {
                continue;
            };
            count += 1;
            // Exactly two index transitions or one lifecycle transition are supported.
            let maximum_count = if name == std::ffi::OsStr::new("lifecycle") {
                1
            } else {
                2
            };
            if count > maximum_count
                || suffix.len() != 32
                || !suffix
                    .iter()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(recovery_publication_conflict());
            }
            let staged = format!(
                ".aura-stage-v2-{}-{}",
                hex::encode(aura_core::hash::hash(name.as_bytes())),
                std::str::from_utf8(suffix).map_err(std::io::Error::other)?
            );
            let (_, bytes) = read_recovery_ciphertext(
                parent,
                std::ffi::OsStr::from_bytes(candidate.to_bytes()),
                maximum,
            )?;
            visit(bytes, staged)?;
        }
        Ok(())
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
    pub(crate) fn acknowledge_entries(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }
    pub(crate) fn names_bounded(
        &self,
        maximum_names: usize,
    ) -> std::io::Result<Vec<std::ffi::OsString>> {
        use std::os::unix::ffi::OsStrExt;
        let mut names = Vec::new();
        for entry in rustix::fs::Dir::read_from(self.file.as_ref())? {
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if bytes != b"." && bytes != b".." {
                if names.len() >= maximum_names {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "profile inventory exceeds bounded reader",
                    ));
                }
                names.push(std::ffi::OsStr::from_bytes(bytes).to_owned());
            }
        }
        Ok(names)
    }
    pub(crate) fn clear_data(&self) -> std::io::Result<()> {
        self.clear_data_inner(true)
    }
    fn clear_data_inner(&self, ordinary_root: bool) -> std::io::Result<()> {
        for name in self.names()? {
            if name == std::ffi::OsStr::new(".aura-profile-owner.lock") {
                continue;
            }
            if ordinary_root
                && name == std::ffi::OsStr::new(crate::profile_storage::SECURE_PROVIDER_DIRECTORY)
            {
                continue;
            }
            match self.child(Path::new(&name), false) {
                Ok(child) => {
                    child.clear_data_inner(false)?;
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
        self.files_inner(false)
    }
    pub(crate) fn ordinary_files(&self) -> std::io::Result<Vec<PathBuf>> {
        self.files_inner(true)
    }
    fn files_inner(&self, ordinary_root: bool) -> std::io::Result<Vec<PathBuf>> {
        let mut result = Vec::new();
        let mut stack = vec![(self.clone(), PathBuf::new())];
        while let Some((directory, prefix)) = stack.pop() {
            for name in directory.names()? {
                if ordinary_root
                    && prefix.as_os_str().is_empty()
                    && name
                        == std::ffi::OsStr::new(crate::profile_storage::SECURE_PROVIDER_DIRECTORY)
                {
                    continue;
                }
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
/// Bounded ciphertext observation; neither deserialization nor observation authorizes mutation.
#[derive(Debug)]
pub(crate) struct ObservedOriginalSuccessor {
    original: Vec<u8>,
    next: Vec<u8>,
    publication: PreparedProfileFile,
}
impl ObservedOriginalSuccessor {
    pub(crate) fn original(&self) -> &[u8] {
        &self.original
    }
    pub(crate) fn next(&self) -> &[u8] {
        &self.next
    }
    pub(crate) fn publication(&self) -> &PreparedProfileFile {
        &self.publication
    }
}

#[derive(Debug)]
pub(crate) struct PreparedProfileFile {
    chain: Vec<ProfileDirectory>,
    name: std::ffi::OsString,
    staged: String,
    observed_digest: Option<[u8; 32]>,
}
impl PreparedProfileFile {
    /// The authenticated initial lifecycle may exclude only its own sole stage.
    pub(crate) fn require_only_entry_in(
        &self,
        directory: &ProfileDirectory,
    ) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing publication directory"))?;
        let expected = parent.file.metadata()?;
        let actual = directory.file.metadata()?;
        let names = directory.names_bounded(2)?;
        if expected.dev() != actual.dev()
            || expected.ino() != actual.ino()
            || names.len() != 1
            || names[0] != std::ffi::OsStr::new(&self.staged)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                StagedPublicationRecoveryError::ConflictingOriginal,
            ));
        }
        Ok(())
    }

    /// Recovery retains the original evidence until an exact existing target is proven.
    pub(crate) fn publish_original(&self, maximum: usize) -> std::io::Result<()> {
        self.publish_original_checked(maximum, || Ok(()))
    }
    #[cfg(test)]
    pub(crate) fn publish_original_with_interleaving(
        &self,
        maximum: usize,
        before_link: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        self.publish_original_checked(maximum, before_link)
    }
    fn publish_original_checked(
        &self,
        maximum: usize,
        before_link: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing publication directory"))?;
        let (source, stage) =
            read_recovery_ciphertext(parent, std::ffi::OsStr::new(&self.staged), maximum)?;
        if self.observed_digest != Some(aura_core::hash::hash(&stage)) {
            return Err(recovery_publication_conflict());
        }
        before_link()?;
        let created = match linkat(
            parent.file.as_ref(),
            &self.staged,
            parent.file.as_ref(),
            &self.name,
            AtFlags::empty(),
        ) {
            Ok(()) => true,
            Err(rustix::io::Errno::EXIST) => false,
            Err(source) => return Err(source.into()),
        };
        // Retain the original opened source through publication, validation and ACK.
        // Path substitution cannot convert a successful link into authenticated evidence.
        let (target, bytes) = read_recovery_ciphertext(parent, &self.name, maximum)?;
        let source_identity = source.metadata()?;
        let target_identity = target.metadata()?;
        let same_inode = source_identity.dev() == target_identity.dev()
            && source_identity.ino() == target_identity.ino();
        if bytes != stage
            || (created && !same_inode)
            || (target_identity.nlink() == 2 && !same_inode)
        {
            return Err(recovery_publication_conflict());
        }
        self.acknowledge()?;
        // The selected profile's exclusive provider lease prevents other authorized
        // writers between verification, target ACK and stage removal.
        unlinkat(parent.file.as_ref(), &self.staged, AtFlags::empty())?;
        self.acknowledge()
    }
    pub(crate) fn require_owner_target(
        &self,
        directory: &ProfileDirectory,
        target: &Path,
    ) -> std::io::Result<()> {
        let (chain, name) = directory.parent(target, false)?;
        let expected = chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing original successor owner"))?;
        let actual = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing observed successor owner"))?;
        if name != self.name || !expected.same_directory(actual)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                StagedPublicationRecoveryError::ForeignPublicationOwner,
            ));
        }
        Ok(())
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "VerifiedOriginalInitializationSuccessor",
        capability_type = crate::secure::VerifiedOriginalInitializationSuccessor,
        family = "runtime_helper"
    )]
    pub(crate) fn publish_verified_initial_successor(
        &self,
        witness: &crate::secure::VerifiedOriginalInitializationSuccessor<'_>,
        maximum: usize,
    ) -> std::io::Result<()> {
        self.publish_initial_cutover_checked(witness, maximum, || Ok(()))
    }
    #[cfg(test)]
    pub(crate) fn publish_initial_cutover_with_interleaving(
        &self,
        witness: &crate::secure::VerifiedOriginalInitializationSuccessor<'_>,
        maximum: usize,
        interleave: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        self.publish_initial_cutover_checked(witness, maximum, interleave)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "VerifiedOriginalInitializationSuccessor",
        family = "runtime_helper"
    )]
    fn require_initial_cutover_custody(
        &self,
        witness: &crate::secure::VerifiedOriginalInitializationSuccessor<'_>,
        maximum: usize,
    ) -> std::io::Result<crate::secure::InitialCutoverIdentities> {
        use std::os::unix::fs::MetadataExt;
        let (old_digest, next_digest) = witness.require_publication(self)?;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("cutover custody parent"))?;
        let before_name = format!(".aura-initial-before-{}", self.staged);
        let next_name = format!(".aura-initial-next-{}", self.staged);
        let identities = witness.require_cutover_journal()?;
        // ACKed journal carries exact original ciphertext and inode identity. Missing
        // custody can only be finished from the same original acknowledged path.
        let (current, current_bytes) =
            read_initial_cutover_ciphertext(parent, &self.name, maximum)?;
        let current_metadata = current.metadata()?;
        let already_exchanged = (current_metadata.dev(), current_metadata.ino()) == identities.next
            && aura_core::hash::hash(&current_bytes) == next_digest;
        for (source_name, custody_name, expected, digest) in [
            (
                self.name.as_os_str(),
                std::ffi::OsStr::new(&before_name),
                identities.original,
                old_digest,
            ),
            (
                std::ffi::OsStr::new(&self.staged),
                std::ffi::OsStr::new(&next_name),
                identities.next,
                next_digest,
            ),
        ] {
            match read_initial_cutover_ciphertext(parent, custody_name, maximum) {
                Ok((file, bytes)) => {
                    let m = file.metadata()?;
                    if (m.dev(), m.ino()) != expected || aura_core::hash::hash(&bytes) != digest {
                        return Err(recovery_publication_conflict());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if already_exchanged {
                        return Err(recovery_publication_conflict());
                    }
                    let (file, bytes) =
                        read_initial_cutover_ciphertext(parent, source_name, maximum)?;
                    let m = file.metadata()?;
                    if (m.dev(), m.ino()) != expected || aura_core::hash::hash(&bytes) != digest {
                        return Err(recovery_publication_conflict());
                    }
                    linkat(
                        parent.file.as_ref(),
                        source_name,
                        parent.file.as_ref(),
                        custody_name,
                        AtFlags::empty(),
                    )?;
                    let (linked, bytes) =
                        read_initial_cutover_ciphertext(parent, custody_name, maximum)?;
                    let m = linked.metadata()?;
                    if (m.dev(), m.ino()) != expected || aura_core::hash::hash(&bytes) != digest {
                        return Err(recovery_publication_conflict());
                    }
                    parent.file.sync_all()?;
                    #[cfg(test)]
                    crate::secure::initialization_cutover_checkpoint(if digest == old_digest {
                        "custody-original"
                    } else {
                        "custody-next"
                    })?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(identities)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "VerifiedOriginalInitializationSuccessor",
        family = "runtime_helper"
    )]
    fn publish_initial_cutover_checked(
        &self,
        witness: &crate::secure::VerifiedOriginalInitializationSuccessor<'_>,
        maximum: usize,
        interleave: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let (old_digest, next_digest) = witness.require_publication(self)?;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("missing cutover parent"))?;
        let identities = self.require_initial_cutover_custody(witness, maximum)?;
        #[cfg(test)]
        crate::secure::initialization_cutover_checkpoint("custody")?;
        let (target, bytes) = read_initial_cutover_ciphertext(parent, &self.name, maximum)?;
        let m = target.metadata()?;
        if (m.dev(), m.ino()) == identities.original && aura_core::hash::hash(&bytes) == old_digest
        {
            interleave()?;
            #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
            rustix::fs::renameat_with(
                parent.file.as_ref(),
                &self.staged,
                parent.file.as_ref(),
                &self.name,
                rustix::fs::RenameFlags::EXCHANGE,
            )?;
            #[cfg(not(any(
                target_os = "linux",
                target_os = "android",
                target_vendor = "apple"
            )))]
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "atomic initial cutover exchange unsupported",
            ));
            #[cfg(test)]
            crate::secure::initialization_cutover_checkpoint("exchange")?;
        } else if (m.dev(), m.ino()) != identities.next
            || aura_core::hash::hash(&bytes) != next_digest
        {
            return Err(recovery_publication_conflict());
        }
        self.acknowledge_initial_cutover_result(witness, maximum)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "VerifiedOriginalInitializationSuccessor",
        family = "runtime_helper"
    )]
    fn acknowledge_initial_cutover_result(
        &self,
        witness: &crate::secure::VerifiedOriginalInitializationSuccessor<'_>,
        maximum: usize,
    ) -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt;
        let (old_digest, next_digest) = witness.require_publication(self)?;
        let identities = witness.require_cutover_journal()?;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("cutover acknowledgment parent"))?;
        let (installed, bytes) = read_initial_cutover_ciphertext(parent, &self.name, maximum)?;
        let m = installed.metadata()?;
        if (m.dev(), m.ino()) != identities.next || aura_core::hash::hash(&bytes) != next_digest {
            return Err(recovery_publication_conflict());
        }
        let displaced_name = format!(".aura-initial-displaced-{}", self.staged);
        let (displaced, bytes) = match read_initial_cutover_ciphertext(
            parent,
            std::ffi::OsStr::new(&self.staged),
            maximum,
        ) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                read_initial_cutover_ciphertext(
                    parent,
                    std::ffi::OsStr::new(&displaced_name),
                    maximum,
                )?
            }
            Err(error) => return Err(error),
        };

        let m = displaced.metadata()?;
        if (m.dev(), m.ino()) != identities.original || aura_core::hash::hash(&bytes) != old_digest
        {
            return Err(recovery_publication_conflict());
        }
        // No unlink precedes full two-direction identity validation and directory ACK.
        self.acknowledge()?;
        #[cfg(test)]
        crate::secure::initialization_cutover_checkpoint("ack")?;
        // Remove no evidence. Archive the displaced original using no-replace rename;
        // an injected foreign replacement is preserved and detected after the move.
        #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
        match rustix::fs::renameat_with(
            parent.file.as_ref(),
            &self.staged,
            parent.file.as_ref(),
            &displaced_name,
            rustix::fs::RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {}
            Err(rustix::io::Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
        let (archived, bytes) = read_initial_cutover_ciphertext(
            parent,
            std::ffi::OsStr::new(&displaced_name),
            maximum,
        )?;
        let m = archived.metadata()?;
        if (m.dev(), m.ino()) != identities.original || aura_core::hash::hash(&bytes) != old_digest
        {
            return Err(recovery_publication_conflict());
        }
        self.acknowledge()
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "VerifiedOriginalInitializationSuccessor",
        family = "runtime_helper"
    )]
    pub(crate) fn archive_initial_cutover_journal(
        &self,
        witness: &crate::secure::VerifiedOriginalInitializationSuccessor<'_>,
    ) -> std::io::Result<()> {
        witness.require_publication(self)?;
        witness.require_cutover_journal()?;
        let path = witness
            .cutover_journal_path()
            .map_err(std::io::Error::other)?;
        let name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("cutover journal name"))?;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("cutover journal parent"))?;
        let (original, original_bytes) = read_recovery_ciphertext(parent, name, 65536)?;
        let history = format!(".aura-initial-history-{}", self.staged);
        #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
        rustix::fs::renameat_with(
            parent.file.as_ref(),
            name,
            parent.file.as_ref(),
            &history,
            rustix::fs::RenameFlags::NOREPLACE,
        )?;
        #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "initial cutover archive unsupported",
        ));
        use std::os::unix::fs::MetadataExt;
        let (archived, archived_bytes) =
            read_recovery_ciphertext(parent, std::ffi::OsStr::new(&history), 65536)?;
        let before = original.metadata()?;
        let after = archived.metadata()?;
        if (before.dev(), before.ino()) != (after.dev(), after.ino())
            || original_bytes != archived_bytes
        {
            return Err(recovery_publication_conflict());
        }
        #[cfg(test)]
        crate::secure::initialization_cutover_checkpoint("archive")?;
        parent.file.sync_all()
    }
    pub(crate) fn initial_cutover_identity(
        &self,
        maximum: usize,
    ) -> std::io::Result<((u64, u64), (u64, u64))> {
        use std::os::unix::fs::MetadataExt;
        let parent = self
            .chain
            .last()
            .ok_or_else(|| std::io::Error::other("cutover parent"))?;
        let (old, _) = read_recovery_ciphertext(parent, &self.name, maximum)?;
        let (next, _) =
            read_recovery_ciphertext(parent, std::ffi::OsStr::new(&self.staged), maximum)?;
        let old = old.metadata()?;
        let next = next.metadata()?;
        Ok(((old.dev(), old.ino()), (next.dev(), next.ino())))
    }
    pub(crate) fn initial_stage_name(&self) -> &str {
        &self.staged
    }

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
                Ok(()) => {
                    #[cfg(test)]
                    post_link_process_checkpoint(&self.name)?;
                }
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

fn recovery_publication_conflict() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        StagedPublicationRecoveryError::ConflictingOriginal,
    )
}
enum RecoveryLinkPolicy {
    OriginalPublicationPair,
    InitialCutoverCustody,
}
fn read_recovery_ciphertext(
    parent: &ProfileDirectory,
    name: &std::ffi::OsStr,
    maximum: usize,
) -> std::io::Result<(File, Vec<u8>)> {
    read_original_ciphertext(
        parent,
        name,
        maximum,
        RecoveryLinkPolicy::OriginalPublicationPair,
    )
}
fn read_initial_cutover_ciphertext(
    parent: &ProfileDirectory,
    name: &std::ffi::OsStr,
    maximum: usize,
) -> std::io::Result<(File, Vec<u8>)> {
    read_original_ciphertext(
        parent,
        name,
        maximum,
        RecoveryLinkPolicy::InitialCutoverCustody,
    )
}
fn read_original_ciphertext(
    parent: &ProfileDirectory,
    name: &std::ffi::OsStr,
    maximum: usize,
    policy: RecoveryLinkPolicy,
) -> std::io::Result<(File, Vec<u8>)> {
    use std::os::unix::fs::MetadataExt;
    parent.require_private()?;
    let file = File::from(openat(
        parent.file.as_ref(),
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?);
    let metadata = file.metadata()?;
    let maximum_links = match policy {
        RecoveryLinkPolicy::OriginalPublicationPair => 2,
        RecoveryLinkPolicy::InitialCutoverCustody => 3,
    };
    if !metadata.is_file()
        || !(1..=maximum_links).contains(&metadata.nlink())
        || metadata.mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            StagedPublicationRecoveryError::InvalidPublicationFile,
        ));
    }
    let maximum = u64::try_from(maximum)
        .map_err(|source| std::io::Error::new(std::io::ErrorKind::InvalidInput, source))?;
    if metadata.len() > maximum {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            StagedPublicationRecoveryError::PublicationTooLarge {
                maximum,
                observed: metadata.len(),
            },
        ));
    }
    let limit = maximum.checked_add(1).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            StagedPublicationRecoveryError::ReadBudgetOverflow,
        )
    })?;
    let mut bytes = Vec::new();
    (&file).take(limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            StagedPublicationRecoveryError::PublicationTooLarge {
                maximum,
                observed: bytes.len() as u64,
            },
        ));
    }
    Ok((file, bytes))
}

// SecureStorageLocation admits namespace/key/optional-subkey: at most two descendant directories.
fn initial_cutover_entry_matches(parent: &Path, name: &std::ffi::OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let target = if parent.as_os_str().is_empty() {
        ".allocation-lifetime-index-v1"
    } else if parent == Path::new(".aura-allocation-lifetime-owner-v1") {
        "lifecycle"
    } else {
        return false;
    };
    let hash = hex::encode(aura_core::hash::hash(target.as_bytes()));
    if name.as_bytes() == format!(".aura-initial-cutover-v1-{hash}").as_bytes() {
        return true;
    }
    for kind in ["before", "next", "displaced", "history"] {
        let prefix = format!(".aura-initial-{kind}-.aura-stage-v2-{hash}-");
        if let Some(suffix) = name.as_bytes().strip_prefix(prefix.as_bytes()) {
            return suffix.len() == 32
                && suffix
                    .iter()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        }
    }
    false
}
fn matches_initial_custody(
    parent: &ProfileDirectory,
    name: &std::ffi::OsStr,
    target: &File,
    bytes: &[u8],
    maximum: usize,
) -> std::io::Result<bool> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    let prefix = format!(
        ".aura-initial-next-.aura-stage-v2-{}-",
        hex::encode(aura_core::hash::hash(name.as_bytes()))
    );
    for entry in rustix::fs::Dir::read_from(parent.file.as_ref())? {
        let entry = entry?;
        let candidate = entry.file_name();
        let Some(suffix) = candidate.to_bytes().strip_prefix(prefix.as_bytes()) else {
            continue;
        };
        if suffix.len() != 32
            || !suffix
                .iter()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(recovery_publication_conflict());
        }
        let candidate_file = File::from(openat(
            parent.file.as_ref(),
            std::ffi::OsStr::from_bytes(candidate.to_bytes()),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )?);
        let candidate_metadata = candidate_file.metadata()?;
        let current_metadata = target.metadata()?;
        if (candidate_metadata.dev(), candidate_metadata.ino())
            != (current_metadata.dev(), current_metadata.ino())
        {
            continue;
        }
        let (custody, retained) = read_recovery_ciphertext(
            parent,
            std::ffi::OsStr::from_bytes(candidate.to_bytes()),
            maximum,
        )?;
        let actual = target.metadata()?;
        let linked = custody.metadata()?;
        if (actual.dev(), actual.ino()) == (linked.dev(), linked.ino()) && bytes == retained {
            return Ok(true);
        }
    }
    Ok(false)
}
const MAX_SECURE_RECORD_DIRECTORY_DEPTH: usize = 2;
fn stage_matches_target(parent: &Path, stage: &std::ffi::OsStr, target: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    if target.parent().unwrap_or(Path::new("")) != parent {
        return false;
    }
    let Some(name) = target.file_name() else {
        return false;
    };
    let prefix = format!(
        ".aura-stage-v2-{}-",
        hex::encode(aura_core::hash::hash(name.as_bytes()))
    );
    let Some(suffix) = stage.to_str().and_then(|stage| stage.strip_prefix(&prefix)) else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
fn post_link_process_checkpoint(name: &std::ffi::OsStr) -> std::io::Result<()> {
    let target = format!("after-link:{}", name.to_string_lossy());
    if std::env::var("AURA_LIFETIME_PRELINK_TARGET")
        .ok()
        .as_deref()
        != Some(target.as_str())
    {
        return Ok(());
    }
    let marker = std::env::var_os("AURA_LIFETIME_PRELINK_MARKER")
        .ok_or_else(|| std::io::Error::other("post-link fixture marker absent"))?;
    std::fs::write(marker, target)?;
    loop {
        std::thread::park();
    }
}
