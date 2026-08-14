use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result};

pub(crate) fn run<I, S>(root: &Path, program: &str, arguments: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let arguments =
        arguments.into_iter().map(|value| value.as_ref().to_owned()).collect::<Vec<_>>();
    writeln!(io::stderr(), "+ {program} {}", display(&arguments))?;
    let status = Command::new(program)
        .args(&arguments)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("cannot run {program}; install the pinned development tools"))?;
    anyhow::ensure!(status.success(), "command failed: {program} {}", display(&arguments));
    Ok(())
}

pub(crate) fn run_env<I, S>(
    root: &Path,
    program: &str,
    arguments: I,
    environment: &[(&str, &str)],
) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let arguments =
        arguments.into_iter().map(|value| value.as_ref().to_owned()).collect::<Vec<_>>();
    writeln!(io::stderr(), "+ {program} {}", display(&arguments))?;
    let status = Command::new(program)
        .args(&arguments)
        .envs(environment.iter().copied())
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("cannot run {program}; install the pinned development tools"))?;
    anyhow::ensure!(status.success(), "command failed: {program} {}", display(&arguments));
    Ok(())
}

pub(crate) fn output<I, S>(root: &Path, program: &str, arguments: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let result = Command::new(program).args(arguments).current_dir(root).output()?;
    anyhow::ensure!(result.status.success(), "command failed: {program}");
    String::from_utf8(result.stdout).context("command output is not UTF-8")
}

fn display(arguments: &[std::ffi::OsString]) -> String {
    arguments.iter().map(|value| value.to_string_lossy()).collect::<Vec<_>>().join(" ")
}
