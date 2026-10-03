//! OS adapter for exclusive lifetime ownership of an existing profile.
//! All cooperating writers must acquire this lock before opening storage.
use async_trait::async_trait;
use aura_core::effects::profile_storage::{
    ProfileStorageEffects, ProfileStorageError, ProfileStorageLease,
};
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::Arc;

#[derive(Debug, Clone)]
/// Resource adapter for the selected profile directory and its lifetime lease.
pub struct FilesystemProfileStorageHandler {
    profile: PathBuf,
}
impl FilesystemProfileStorageHandler {
    /// Describe the selected path without creating a directory, lock or identity.
    /// Acquisition may create an empty missing directory before locking it;
    /// it never initializes profile data or secrets before exclusive ownership.
    pub fn new(profile: PathBuf) -> Self {
        Self { profile }
    }
}

#[cfg(unix)]
#[derive(Debug)]
/// Actual adapter lease: fields are private and there is no default/no-op mint.
/// ```compile_fail
/// use aura_effects::profile_storage::OwnedProfileLease;
/// let forged=OwnedProfileLease {};
/// ```
pub struct OwnedProfileLease {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd"
    ))]
    pub(crate) keyring_namespace:
        std::sync::OnceLock<Arc<crate::platform_namespace::PlatformNamespaceLease>>,
    _file: std::fs::File,
    pub(crate) directory: crate::profile_directory::ProfileDirectory,
    identity: String,
}
#[cfg(unix)]
impl ProfileStorageLease for OwnedProfileLease {
    fn profile_identity(&self) -> &str {
        &self.identity
    }
}
// No explicit unlock: closing the sole owned descriptor releases flock.
// The stable lock inode must NEVER be unlinked, renamed or replaced.

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ProfileStorageEffects for FilesystemProfileStorageHandler {
    async fn acquire_profile_lease(
        &self,
    ) -> Result<Box<dyn ProfileStorageLease>, ProfileStorageError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.acquire_owned_native()
                .map(|lease| Box::new(lease) as Box<dyn ProfileStorageLease>)
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.acquire_owned_browser()
                .await
                .map(|lease| Box::new(lease) as Box<dyn ProfileStorageLease>)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FilesystemProfileStorageHandler {
    /// Concrete audited producer used by synchronous native assembly. Not a
    /// generic trait guard: its private token cannot be replaced by a no-op.
    pub fn acquire_owned_native(&self) -> Result<OwnedProfileLease, ProfileStorageError> {
        #[cfg(unix)]
        {
            use rustix::fs::{flock, openat, FlockOperation, Mode, OFlags, CWD};
            let io = |source: std::io::Error| ProfileStorageError::Io {
                source: Arc::new(source),
            };
            match std::fs::symlink_metadata(&self.profile) {
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir_all(&self.profile).map_err(io)?;
                }
                Err(source) => return Err(io(source)),
                Ok(_) => {}
            }
            let metadata = std::fs::symlink_metadata(&self.profile).map_err(io)?;

            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ProfileStorageError::Invalid(
                    "profile must be an existing real directory".into(),
                ));
            }
            let canonical = std::fs::canonicalize(&self.profile).map_err(io)?;
            let directory = openat(
                CWD,
                &canonical,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| io(e.into()))?;
            let descriptor = openat(
                &directory,
                ".aura-profile-owner.lock",
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|e| io(e.into()))?;
            let file: std::fs::File = descriptor.into();
            let lock_metadata = file.metadata().map_err(io)?;
            use std::os::unix::fs::MetadataExt;
            if !lock_metadata.is_file()
                || lock_metadata.mode() & 0o077 != 0
                || lock_metadata.nlink() != 1
            {
                return Err(ProfileStorageError::Invalid(
                    "lock must be a private unlinked-alias-free regular file".into(),
                ));
            }
            flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|e| {
                if e == rustix::io::Errno::WOULDBLOCK {
                    ProfileStorageError::Busy
                } else {
                    io(e.into())
                }
            })?;
            // Name-to-inode validation prevents acquiring an obsolete replaced
            // inode during a cooperating setup race. Adversarial same-user path
            // replacement is outside advisory locking's security guarantee.
            let named = std::fs::symlink_metadata(canonical.join(".aura-profile-owner.lock"))
                .map_err(io)?;
            if named.dev() != lock_metadata.dev() || named.ino() != lock_metadata.ino() {
                return Err(ProfileStorageError::Invalid(
                    "lock inode changed during acquisition".into(),
                ));
            }
            // Sync new lock directory entry; durable profile data is separately
            // synced by its storage transaction, never by this ownership guard.
            let directory = std::fs::File::from(directory);
            directory.sync_all().map_err(io)?;
            let directory = crate::profile_directory::ProfileDirectory::from_file(directory);
            let identity = canonical
                .to_str()
                .ok_or_else(|| ProfileStorageError::Invalid("profile path is not UTF-8".into()))?
                .to_owned();
            Ok(OwnedProfileLease {
                #[cfg(any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "linux",
                    target_os = "freebsd",
                    target_os = "openbsd"
                ))]
                keyring_namespace: std::sync::OnceLock::new(),
                _file: file,
                directory,
                identity,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = &self.profile;
            Err(ProfileStorageError::Unsupported)
        }
    }
}

#[cfg(all(not(unix), not(target_arch = "wasm32")))]
#[derive(Debug)]
pub struct OwnedProfileLease {
    _uninhabited: std::convert::Infallible,
}
#[cfg(all(not(unix), not(target_arch = "wasm32")))]
impl ProfileStorageLease for OwnedProfileLease {
    fn profile_identity(&self) -> &str {
        match self._uninhabited {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl OwnedProfileLease {
    /// Confirm the writer's physical directory matches this lifetime owner.
    pub fn matches_profile(&self, path: &std::path::Path) -> Result<bool, ProfileStorageError> {
        let canonical = std::fs::canonicalize(path).map_err(|source| ProfileStorageError::Io {
            source: std::sync::Arc::new(source),
        })?;
        Ok(canonical.to_str() == Some(self.profile_identity()))
    }
}

#[cfg(target_arch = "wasm32")]
pub struct OwnedProfileLease {
    identity: String,
    release: Option<futures::channel::oneshot::Sender<()>>,
    completion: Option<futures::channel::oneshot::Receiver<Result<(), ProfileStorageError>>>,
}
#[cfg(target_arch = "wasm32")]
impl std::fmt::Debug for OwnedProfileLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedProfileLease")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
#[cfg(target_arch = "wasm32")]
impl ProfileStorageLease for OwnedProfileLease {
    fn profile_identity(&self) -> &str {
        &self.identity
    }
}
#[cfg(target_arch = "wasm32")]
impl Drop for OwnedProfileLease {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
#[cfg(target_arch = "wasm32")]
impl OwnedProfileLease {
    pub fn matches_profile(&self, path: &std::path::Path) -> Result<bool, ProfileStorageError> {
        Ok(path.to_str() == Some(self.identity.as_str()))
    }
    /// Call only after persistent writers and runtime tasks acknowledge shutdown.
    /// Drop/cancellation still releases; explicit release observes broker completion.
    pub async fn release(mut self) -> Result<(), ProfileStorageError> {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(completion) = self.completion.take() {
            match futures::future::select(
                Box::pin(completion),
                Box::pin(gloo_timers::future::TimeoutFuture::new(5_000)),
            )
            .await
            {
                futures::future::Either::Left((outcome, _)) => outcome.map_err(|_| {
                    ProfileStorageError::Invalid("lock completion acknowledgement cancelled".into())
                })??,
                futures::future::Either::Right((_, _)) => {
                    return Err(ProfileStorageError::Timeout {
                        operation: "browser profile release acknowledgement",
                    })
                }
            }
        }
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn browser_profile_error(
    operation: &'static str,
    error: wasm_bindgen::JsValue,
) -> ProfileStorageError {
    // Explicit foreign diagnostic boundary: JsValue is not a process-local
    // Send+Sync Rust Error. Preserve typed operation/name/message, never retry
    // or classify a business outcome using diagnostic text.
    let name = js_sys::Reflect::get(&error, &"name".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "JavaScriptError".into());
    let message = js_sys::Reflect::get(&error, &"message".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| format!("{error:?}"));
    ProfileStorageError::Browser {
        operation,
        name,
        message,
    }
}

#[cfg(target_arch = "wasm32")]
impl FilesystemProfileStorageHandler {
    pub async fn acquire_owned_browser(&self) -> Result<OwnedProfileLease, ProfileStorageError> {
        use futures::future::{select, Either};
        use wasm_bindgen::{closure::Closure, JsCast, JsValue};
        use wasm_bindgen_futures::{future_to_promise, JsFuture};
        if cfg!(target_feature = "atomics") {
            return Err(ProfileStorageError::Unsupported);
        }
        let identity = self
            .profile
            .to_str()
            .filter(|s| !s.is_empty() && s.len() <= 4096)
            .ok_or_else(|| {
                ProfileStorageError::Invalid("invalid browser storage namespace".into())
            })?
            .to_owned();
        let window = web_sys::window().ok_or(ProfileStorageError::Unsupported)?;
        let navigator = js_sys::Reflect::get(window.as_ref(), &"navigator".into())
            .map_err(|e| browser_profile_error("navigator", e))?;
        let locks = js_sys::Reflect::get(&navigator, &"locks".into())
            .map_err(|e| browser_profile_error("locks", e))?;
        if locks.is_null() || locks.is_undefined() {
            return Err(ProfileStorageError::Unsupported);
        }
        let request = js_sys::Reflect::get(&locks, &"request".into())
            .map_err(|e| browser_profile_error("request lookup", e))?
            .dyn_into::<js_sys::Function>()
            .map_err(|e| browser_profile_error("request function", e))?;
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"mode".into(), &"exclusive".into())
            .map_err(|e| browser_profile_error("lock options", e))?;
        js_sys::Reflect::set(&options, &"ifAvailable".into(), &JsValue::TRUE)
            .map_err(|e| browser_profile_error("lock options", e))?;
        let (admit, admitted) =
            futures::channel::oneshot::channel::<Result<(), ProfileStorageError>>();
        let (release, released) = futures::channel::oneshot::channel::<()>();
        let mut admit = Some(admit);
        let mut released = Some(released);
        let callback = Closure::wrap(Box::new(move |lock: JsValue| -> js_sys::Promise {
            let Some(admit) = admit.take() else {
                return js_sys::Promise::reject(&JsValue::from_str("lock callback repeated"));
            };
            if lock.is_null() || lock.is_undefined() {
                let _ = admit.send(Err(ProfileStorageError::Busy));
                return js_sys::Promise::resolve(&JsValue::UNDEFINED);
            }
            let Some(released) = released.take() else {
                return js_sys::Promise::reject(&JsValue::from_str("lock release owner absent"));
            };
            let _ = admit.send(Ok(()));
            future_to_promise(async move {
                let _ = released.await;
                Ok(JsValue::UNDEFINED)
            })
        }) as Box<dyn FnMut(JsValue) -> js_sys::Promise>);
        let promise = request
            .call3(
                &locks,
                &JsValue::from_str(&format!("aura-profile-owner:{identity}")),
                options.as_ref(),
                callback.as_ref(),
            )
            .map_err(|e| browser_profile_error("request", e))?
            .dyn_into::<js_sys::Promise>()
            .map_err(|e| browser_profile_error("request promise", e))?;
        let (completed, completion) =
            futures::channel::oneshot::channel::<Result<(), ProfileStorageError>>();
        // Adapter-local OS request completion. Its sole release signal is owned
        // by the returned Rust token; no product task or semantic lifecycle runs
        // here. Always observe rejection, including admission cancellation.
        let _observed = future_to_promise(async move {
            let outcome = JsFuture::from(promise)
                .await
                .map(|_| ())
                .map_err(|e| browser_profile_error("request completion", e));
            let _ = completed.send(outcome);
            Ok(JsValue::UNDEFINED)
        });

        let ownership = match select(
            Box::pin(select(admitted, completion)),
            Box::pin(gloo_timers::future::TimeoutFuture::new(5_000)),
        )
        .await
        {
            Either::Left((ownership, _)) => ownership,
            Either::Right((_, _)) => {
                return Err(ProfileStorageError::Timeout {
                    operation: "browser profile acquisition",
                })
            }
        };
        match ownership {
            Either::Left((admission, completion)) => {
                admission.map_err(|_| {
                    ProfileStorageError::Invalid("lock callback admission cancelled".into())
                })??;
                // Callback has completed synchronously; its returned promise owns
                // release waiting. It is safe to drop the Rust callback now.
                drop(callback);
                Ok(OwnedProfileLease {
                    identity,
                    release: Some(release),
                    completion: Some(completion),
                })
            }
            Either::Right((completion, _)) => {
                completion.map_err(|_| {
                    ProfileStorageError::Invalid("request acknowledgement cancelled".into())
                })??;
                Err(ProfileStorageError::Invalid(
                    "lock request completed without admission".into(),
                ))
            }
        }
    }
}

/// Create directory components relative to held directory descriptors. Existing
/// symlinks are rejected before any descendants are created. The returned path
/// anchors relative callers to their acquisition-time working directory.
#[cfg(unix)]
pub(crate) fn create_contained_directory(
    base: &std::path::Path,
    relative: &std::path::Path,
) -> std::io::Result<PathBuf> {
    use rustix::fs::{mkdirat, openat, Mode, OFlags, CWD};
    use std::path::Component;
    let mut anchored = std::fs::canonicalize(base)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = openat(CWD, &anchored, flags, Mode::empty())?;
    for component in relative.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(name) => {
                match openat(&directory, name, flags, Mode::empty()) {
                    Ok(next) => directory = next,
                    Err(rustix::io::Errno::NOENT) => {
                        match mkdirat(&directory, name, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                            Err(error) => return Err(error.into()),
                        }
                        directory = openat(&directory, name, flags, Mode::empty())?;
                    }
                    Err(error) => return Err(error.into()),
                }
                anchored.push(name);
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "storage descendant must contain only normal path components",
                ))
            }
        }
    }
    Ok(anchored)
}

#[cfg(all(test, unix))]
mod contained_directory_tests {
    #[test]
    fn foreign_symlink_rejected_before_descendant_creation() -> std::io::Result<()> {
        let profile = tempfile::tempdir()?;
        let foreign = tempfile::tempdir()?;
        std::os::unix::fs::symlink(foreign.path(), profile.path().join("foreign"))?;
        assert!(super::create_contained_directory(
            profile.path(),
            std::path::Path::new("foreign/new/secret")
        )
        .is_err());
        assert!(!foreign.path().join("new").exists());
        Ok(())
    }

    #[test]
    fn creates_descendants_under_real_profile() -> std::io::Result<()> {
        let profile = tempfile::tempdir()?;
        let created = super::create_contained_directory(
            profile.path(),
            std::path::Path::new("nested/actual"),
        )?;
        assert_eq!(
            created,
            std::fs::canonicalize(profile.path())?.join("nested/actual")
        );
        assert!(created.is_dir());
        Ok(())
    }
}
