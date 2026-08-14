use std::path::Path;

use anyhow::Result;

use crate::command::run;

pub(crate) fn check(root: &Path, hours: u64) -> Result<()> {
    anyhow::ensure!(hours >= 8, "release soak requires at least 8 hours");
    run(root, "cargo", ["build", "--release", "--locked", "--package", "eas-mail-mcp"])?;
    run(
        root,
        "cargo",
        [
            "build",
            "--release",
            "--locked",
            "--package",
            "eas-mail-mcp-harness",
            "--features",
            "soak",
            "--bin",
            "soak-harness",
        ],
    )?;
    let harness = root.join("target/release/soak-harness");
    let application = root.join("target/release/eas-mail-mcp");
    let executable = harness.to_str().ok_or_else(|| anyhow::anyhow!("soak path is not UTF-8"))?;
    run(
        root,
        executable,
        [
            "--application".as_ref(),
            application.as_os_str(),
            "--hours".as_ref(),
            hours.to_string().as_ref(),
        ],
    )
}
