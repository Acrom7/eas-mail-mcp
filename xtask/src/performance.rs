use std::path::Path;

use anyhow::Result;

use crate::command::run;

pub(crate) fn check(root: &Path, python: &str) -> Result<()> {
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
            "performance",
            "--bin",
            "perf-harness",
            "--bin",
            "perf-server",
        ],
    )?;
    let harness = root.join("target/release/perf-harness");
    let application = root.join("target/release/eas-mail-mcp");
    let baseline = root.join("benchmarks/python_stdio_baseline.py");
    let executable =
        harness.to_str().ok_or_else(|| anyhow::anyhow!("performance harness path is not UTF-8"))?;
    run(
        root,
        executable,
        [
            "--application".as_ref(),
            application.as_os_str(),
            "--python-baseline".as_ref(),
            baseline.as_os_str(),
            "--python".as_ref(),
            python.as_ref(),
        ],
    )
}
