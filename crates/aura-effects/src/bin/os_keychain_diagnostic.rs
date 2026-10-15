//! Explicit isolated-account probe of the selected native Keychain adapter.
//! This binary is never part of a deployable feature graph.

#[cfg(target_os = "macos")]
mod macos {
    use aura_core::effects::secure::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    use aura_effects::profile_storage::FilesystemProfileStorageHandler;
    use aura_effects::secure::ProductionSecureStorageHandler;
    use rand::RngCore;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    const MARKER: &str = "/private/var/db/aura/keychain-diagnostic-identity";
    const VALUE: &[u8] = b"isolated-os-keychain-diagnostic-v1";

    fn private_component(path: &Path, owner: u32, directory: bool) -> Result<(), &'static str> {
        let meta = std::fs::symlink_metadata(path).map_err(|_| "marker_missing")?;
        if meta.file_type().is_symlink()
            || meta.uid() != owner
            || meta.permissions().mode() & 0o022 != 0
            || (directory && !meta.is_dir())
            || (!directory && !meta.is_file())
        {
            return Err("marker_unsafe");
        }
        Ok(())
    }

    pub fn preflight() -> Result<(), &'static str> {
        let uid = rustix::process::getuid().as_raw();
        let euid = rustix::process::geteuid().as_raw();
        if uid == 0 || uid != euid {
            return Err("isolation_identity");
        }
        for path in [
            "/private",
            "/private/var",
            "/private/var/db",
            "/private/var/db/aura",
        ] {
            private_component(Path::new(path), 0, true)?;
        }
        private_component(Path::new(MARKER), 0, false)?;
        let bytes = std::fs::read(MARKER).map_err(|_| "marker_missing")?;
        if bytes.len() > 128 {
            return Err("marker_unsafe");
        }
        let contents = std::str::from_utf8(&bytes).map_err(|_| "marker_unsafe")?;
        let mut lines = contents.lines();
        let isolated = lines
            .next()
            .and_then(|line| line.strip_prefix("isolated_uid="))
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or("marker_unsafe")?;
        let developer = lines
            .next()
            .and_then(|line| line.strip_prefix("developer_uid="))
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or("marker_unsafe")?;
        if lines.next().is_some() || isolated != uid || developer == uid {
            return Err("isolation_identity");
        }
        Ok(())
    }

    #[derive(Debug)]
    pub enum Stage {
        Profile,
        Selection,
        Entropy,
        Write,
        Readback,
        Reopen,
        Cleanup,
    }
    impl Stage {
        pub fn code(&self) -> &'static str {
            match self {
                Self::Profile => "profile_lease",
                Self::Selection => "selected_provider",
                Self::Entropy => "diagnostic_entropy",
                Self::Write => "provider_write",
                Self::Readback => "provider_readback",
                Self::Reopen => "same_profile_reopen",
                Self::Cleanup => "provider_cleanup",
            }
        }
    }

    pub async fn run(profile: PathBuf) -> Result<(), Stage> {
        // Repeat the binary's first check at the provider boundary. No Keychain
        // entry can be constructed before both observations pass.
        preflight().map_err(|_| Stage::Selection)?;
        let meta = std::fs::symlink_metadata(&profile).map_err(|_| Stage::Profile)?;
        if meta.file_type().is_symlink()
            || !meta.is_dir()
            || meta.uid() != rustix::process::geteuid().as_raw()
            || meta.permissions().mode() & 0o077 != 0
            || std::fs::read_dir(&profile)
                .map_err(|_| Stage::Profile)?
                .next()
                .is_some()
        {
            return Err(Stage::Profile);
        }
        let owner = Arc::new(
            FilesystemProfileStorageHandler::new(profile.clone())
                .acquire_owned_native()
                .map_err(|_| Stage::Profile)?,
        );
        let selected = ProductionSecureStorageHandler::for_production(profile.clone());
        if !matches!(selected, ProductionSecureStorageHandler::Platform(_)) {
            return Err(Stage::Selection);
        }
        let backend = selected
            .retain_profile_owner(owner.clone())
            .map_err(|_| Stage::Selection)?;
        let mut random = [0_u8; 16];
        rand::rngs::OsRng
            .try_fill_bytes(&mut random)
            .map_err(|_| Stage::Entropy)?;
        let location = SecureStorageLocation::new("os_keychain_diagnostic_v1", hex::encode(random));
        let capabilities = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
            SecureStorageCapability::Delete,
        ];
        backend
            .secure_store(&location, VALUE, &capabilities)
            .await
            .map_err(|_| Stage::Write)?;
        if backend
            .secure_retrieve(&location, &capabilities)
            .await
            .map_err(|_| Stage::Readback)?
            != VALUE
        {
            return Err(Stage::Readback);
        }
        drop(backend);
        drop(owner);
        let reopened = Arc::new(
            FilesystemProfileStorageHandler::new(profile.clone())
                .acquire_owned_native()
                .map_err(|_| Stage::Reopen)?,
        );
        let backend = ProductionSecureStorageHandler::for_production(profile)
            .retain_profile_owner(reopened.clone())
            .map_err(|_| Stage::Reopen)?;
        if backend
            .secure_retrieve(&location, &capabilities)
            .await
            .map_err(|_| Stage::Reopen)?
            != VALUE
        {
            return Err(Stage::Reopen);
        }
        backend
            .secure_delete(&location, &capabilities)
            .await
            .map_err(|_| Stage::Cleanup)?;
        if backend
            .secure_exists(&location)
            .await
            .map_err(|_| Stage::Cleanup)?
        {
            return Err(Stage::Cleanup);
        }
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args_os();
    let _binary = args.next();
    let profile = match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None) if flag == "--profile" => std::path::PathBuf::from(path),
        _ => {
            println!("{{\"status\":\"refused\",\"class\":\"invalid_arguments\"}}");
            std::process::exit(64);
        }
    };
    #[cfg(target_os = "macos")]
    {
        // Unit-test and mock-keyring builds cannot provide OS-service evidence.
        if cfg!(any(test, feature = "test-keyring")) {
            println!("{{\"status\":\"refused\",\"class\":\"mock_provider_feature\"}}");
            std::process::exit(78);
        }
        if let Err(class) = macos::preflight() {
            println!("{{\"status\":\"refused\",\"class\":\"{class}\"}}");
            std::process::exit(78);
        }
        match macos::run(profile).await {
            Ok(()) => {
                println!("{{\"status\":\"passed\",\"class\":\"same_profile_selected_provider\"}}")
            }
            Err(stage) => {
                println!("{{\"status\":\"failed\",\"class\":\"{}\"}}", stage.code());
                std::process::exit(1);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = profile;
        println!("{{\"status\":\"refused\",\"class\":\"unsupported_platform\"}}");
        std::process::exit(78);
    }
}
