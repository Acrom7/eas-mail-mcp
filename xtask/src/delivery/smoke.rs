use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context as _, Result};

const BINARY: &str = "eas-mail-mcp";

pub(super) fn verify(dist: &Path, bundle: &Path, target: &str) -> Result<()> {
    let architecture = if target.starts_with("aarch64") { "arm64" } else { "x86_64" };
    let root = dist.join(format!(".install-smoke-{architecture}"));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    let result = verify_in_root(&root, bundle, architecture);
    let cleanup = fs::remove_dir_all(&root);
    result?;
    cleanup?;
    Ok(())
}

fn verify_in_root(root: &Path, bundle: &Path, architecture: &str) -> Result<()> {
    let home = root.join("home");
    let base = root.join("lib");
    let bin = root.join("bin");
    fs::create_dir_all(&home)?;

    run(architecture, &bundle.join("install.sh"), &[], &home, &base, &bin)?;
    let installed = bin.join(BINARY);
    anyhow::ensure!(fs::symlink_metadata(&installed)?.file_type().is_symlink());
    let version = run(architecture, &installed, &["--version"], &home, &base, &bin)?;
    anyhow::ensure!(
        String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")),
        "installed {architecture} binary returned an unexpected version"
    );

    let support = home.join("Library/Application Support/EAS Mail MCP");
    fs::write(support.join("config.toml"), "smoke_test = true\n")?;
    run(architecture, &bundle.join("install.sh"), &[], &home, &base, &bin)?;
    anyhow::ensure!(contains_upgrade_backup(&support.join("Install Backups"))?);

    let uninstall = base.join(env!("CARGO_PKG_VERSION")).join("share/uninstall.sh");
    run(architecture, &uninstall, &[], &home, &base, &bin)?;
    ensure_absent(&installed)?;
    anyhow::ensure!(support.join("config.toml").is_file(), "uninstall removed user data");
    Ok(())
}

fn ensure_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => anyhow::bail!("uninstall retained {}", path.display()),
    }
}

fn contains_upgrade_backup(directory: &Path) -> Result<bool> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if regular_file(&path.join("config.toml")) && regular_file(&path.join("previous-binary")) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn run(
    architecture: &str,
    program: &Path,
    arguments: &[&str],
    home: &Path,
    base: &Path,
    bin: &Path,
) -> Result<Output> {
    let output = Command::new("arch")
        .arg(format!("-{architecture}"))
        .arg(program)
        .args(arguments)
        .env("HOME", home)
        .env("EAS_MAIL_MCP_HOME", base)
        .env("EAS_MAIL_MCP_BIN_DIR", bin)
        .output()
        .with_context(|| format!("cannot run {} installer smoke", program.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "{} installer smoke failed: {}",
        architecture,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output)
}
