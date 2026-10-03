//! Service-wide custody for the original OS keyring namespace.
//! Physical profile ownership alone cannot serialize a service shared by profiles.
use aura_core::effects::profile_storage::ProfileStorageError;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct PlatformNamespaceLease {
    _owner: crate::profile_storage::OwnedProfileLease,
    service: String,
    mutation: tokio::sync::Mutex<()>,
}
impl PlatformNamespaceLease {
    /// Original selected-owner sharing preserves the exact service lease through
    /// sanctioned reassembly; creating an unrelated filesystem profile cannot
    /// manufacture a second writer for the shared keyring service.
    pub(crate) fn for_selected_profile(
        owner: &crate::profile_storage::OwnedProfileLease,
        service: &str,
    ) -> Result<Arc<Self>, ProfileStorageError> {
        #[cfg(unix)]
        {
            if let Some(existing) = owner.keyring_namespace.get() {
                return Self::matching(existing, service);
            }
            match Self::acquire(service) {
                Ok(lease) => {
                    if owner.keyring_namespace.set(lease.clone()).is_ok() {
                        Ok(lease)
                    } else {
                        owner
                            .keyring_namespace
                            .get()
                            .ok_or_else(|| {
                                ProfileStorageError::Invalid(
                                    "keyring custody disappeared during attachment".into(),
                                )
                            })
                            .and_then(|existing| Self::matching(existing, service))
                    }
                }
                Err(error) => owner
                    .keyring_namespace
                    .get()
                    .ok_or(error)
                    .and_then(|existing| Self::matching(existing, service)),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (owner, service);
            Err(ProfileStorageError::Unsupported)
        }
    }
    fn matching(existing: &Arc<Self>, service: &str) -> Result<Arc<Self>, ProfileStorageError> {
        if existing.service != service {
            return Err(ProfileStorageError::Invalid(
                "selected owner is already bound to another keyring service".into(),
            ));
        }
        Ok(existing.clone())
    }
    pub(crate) fn acquire(service: &str) -> Result<Arc<Self>, ProfileStorageError> {
        #[cfg(unix)]
        {
            use rustix::fs::{fstat, openat, Mode, OFlags, CWD};
            let io = |error: std::io::Error| ProfileStorageError::Io {
                source: Arc::new(error),
            };
            // Fixed OS namespace, never caller HOME/TMPDIR/XDG or selected profile.
            // macOS resolves /tmp to /private/tmp; the physical root must be the
            // actual root-owned sticky temporary directory before creating a child.
            let physical_root = std::fs::canonicalize("/tmp").map_err(io)?;
            let root = openat(
                CWD,
                &physical_root,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| io(error.into()))?;
            let stat = fstat(&root).map_err(|error| io(error.into()))?;
            if stat.st_uid != 0 || stat.st_mode & 0o1000 == 0 {
                return Err(ProfileStorageError::Invalid(
                    "keyring owner root is not the OS root-owned sticky directory".into(),
                ));
            }
            let uid = rustix::process::geteuid().as_raw();
            let digest = aura_core::hash::hash(service.as_bytes());
            let suffix = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let name = format!("aura-keyring-owner-{uid}-{suffix}");
            let directory =
                crate::profile_directory::ProfileDirectory::from_file(std::fs::File::from(root));
            let owned_directory = directory
                .child(std::path::Path::new(&name), true)
                .map_err(io)?;
            owned_directory.require_private().map_err(io)?;
            // UID validation is independent of permissions: another local user
            // cannot precreate an apparently private namespace and mint our lease.
            use std::os::unix::fs::MetadataExt;
            let named = std::fs::symlink_metadata(physical_root.join(&name)).map_err(io)?;
            if named.uid() != uid || !named.is_dir() || named.file_type().is_symlink() {
                return Err(ProfileStorageError::Invalid(
                    "keyring owner directory belongs to another OS identity".into(),
                ));
            }
            let owner = crate::profile_storage::FilesystemProfileStorageHandler::new(
                physical_root.join(name),
            )
            .acquire_owned_native()?;
            Ok(Arc::new(Self {
                _owner: owner,
                service: service.to_owned(),
                mutation: tokio::sync::Mutex::new(()),
            }))
        }
        #[cfg(not(unix))]
        {
            let _ = service;
            Err(ProfileStorageError::Unsupported)
        }
    }
    pub(crate) async fn mutation_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.mutation.lock().await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
    #[test]
    fn namespace_child_observes_os_custody() -> TestResult {
        let Some(service) = std::env::var_os("AURA_KEYRING_NAMESPACE_CHILD") else {
            return Ok(());
        };
        let service = service.to_str().ok_or("test service is not UTF-8")?;
        if std::env::var_os("AURA_KEYRING_NAMESPACE_HOLD").is_some() {
            use std::io::Write;
            let _owned = PlatformNamespaceLease::acquire(service)?;
            std::io::stdout().write_all(b"owned\n")?;
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            return Ok(());
        }
        let acquired = PlatformNamespaceLease::acquire(service);
        if std::env::var_os("AURA_KEYRING_NAMESPACE_EXPECT_BUSY").is_some() {
            assert!(matches!(acquired, Err(ProfileStorageError::Busy)));
        } else {
            assert!(acquired.is_ok());
        }
        Ok(())
    }
    #[test]
    fn original_owner_retains_one_service_lease_across_views_and_processes() -> TestResult {
        let profile = tempfile::tempdir()?;
        let other = tempfile::tempdir()?;
        // Private fixture namespace; these tests never access real keyring records.
        let service = format!("aura-owner-fixture:{}", profile.path().display());
        let owner = Arc::new(
            crate::profile_storage::FilesystemProfileStorageHandler::new(profile.path().to_owned())
                .acquire_owned_native()?,
        );
        let namespace = PlatformNamespaceLease::for_selected_profile(&owner, &service)?;
        let inherited = PlatformNamespaceLease::for_selected_profile(&owner, &service)?;
        assert!(Arc::ptr_eq(&namespace, &inherited));
        assert!(matches!(
            PlatformNamespaceLease::for_selected_profile(&owner, "another-service"),
            Err(ProfileStorageError::Invalid(_))
        ));
        let foreign =
            crate::profile_storage::FilesystemProfileStorageHandler::new(other.path().to_owned())
                .acquire_owned_native()?;
        assert!(matches!(
            PlatformNamespaceLease::for_selected_profile(&foreign, &service),
            Err(ProfileStorageError::Busy)
        ));
        let child = |busy: bool| -> TestResult {
            let mut command = std::process::Command::new(std::env::current_exe()?);
            command
                .args([
                    "--exact",
                    "platform_namespace::tests::namespace_child_observes_os_custody",
                    "--nocapture",
                ])
                .env("AURA_KEYRING_NAMESPACE_CHILD", &service);
            if busy {
                command.env("AURA_KEYRING_NAMESPACE_EXPECT_BUSY", "1");
            } else {
                command.env_remove("AURA_KEYRING_NAMESPACE_EXPECT_BUSY");
            }
            assert!(command.status()?.success());
            Ok(())
        };
        child(true)?;
        drop(inherited);
        drop(namespace);
        drop(owner);
        child(false)?;
        Ok(())
    }
    #[test]
    fn actual_process_death_releases_shared_service_owner() -> TestResult {
        use std::io::BufRead;
        let marker = tempfile::tempdir()?;
        let service = format!("aura-crash-owner-fixture:{}", marker.path().display());
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "platform_namespace::tests::namespace_child_observes_os_custody",
                "--nocapture",
            ])
            .env("AURA_KEYRING_NAMESPACE_CHILD", &service)
            .env("AURA_KEYRING_NAMESPACE_HOLD", "1")
            .env_remove("AURA_KEYRING_NAMESPACE_EXPECT_BUSY")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let output = child.stdout.take().ok_or("child has no owner signal")?;
        let mut output = std::io::BufReader::new(output);
        let mut owned = false;
        loop {
            let mut line = String::new();
            if output.read_line(&mut line)? == 0 {
                break;
            }
            if line.trim() == "owned" {
                owned = true;
                break;
            }
        }
        assert!(owned, "child must acknowledge actual descriptor custody");
        assert!(matches!(
            PlatformNamespaceLease::acquire(&service),
            Err(ProfileStorageError::Busy)
        ));
        child.kill()?;
        child.wait()?;
        let _recovered = PlatformNamespaceLease::acquire(&service)?;
        Ok(())
    }
}
