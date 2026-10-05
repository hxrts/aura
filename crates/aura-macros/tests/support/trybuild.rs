use aura_build_support as process_lock;

pub(crate) fn acquire_trybuild_lock(
) -> Result<process_lock::TrybuildProcessLock, Box<dyn std::error::Error>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("required Cargo failed: {status}")).into());
    }
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| std::io::Error::other("missing workspace root"))?;
    Ok(process_lock::TrybuildProcessLock::acquire_workspace(
        workspace,
        std::time::Duration::from_secs(900),
    )?)
}
