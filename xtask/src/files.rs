use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::Result;

const WARNING_LINES: usize = 300;
const MAXIMUM_LINES: usize = 500;

pub(crate) fn check(root: &Path) -> Result<()> {
    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    for path in rust_files(root)? {
        let lines = physical_lines(&path)?;
        if lines > MAXIMUM_LINES {
            failures.push((path, lines));
        } else if lines > WARNING_LINES {
            warnings.push((path, lines));
        }
    }
    for (path, lines) in warnings {
        writeln!(io::stderr(), "warning: {} has {lines} lines", relative(root, &path))?;
    }
    for (path, lines) in &failures {
        writeln!(io::stderr(), "error: {} has {lines} lines", relative(root, path))?;
    }
    anyhow::ensure!(failures.is_empty(), "handwritten Rust files must not exceed 500 lines");
    Ok(())
}

pub(crate) fn text_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut output = Vec::new();
    visit(root, root, &mut |path| {
        if text_extension(path) {
            output.push(path.to_owned());
        }
        Ok(())
    })?;
    Ok(output)
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut output = Vec::new();
    visit(root, root, &mut |path| {
        if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path.to_owned());
        }
        Ok(())
    })?;
    Ok(output)
}

fn visit(
    root: &Path,
    directory: &Path,
    action: &mut impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if !excluded(root, &path) {
                visit(root, &path, action)?;
            }
        } else if metadata.is_file() {
            action(&path)?;
        }
    }
    Ok(())
}

fn excluded(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).ok().is_some_and(|relative| {
        relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some(
                    ".git"
                        | ".private"
                        | ".venv"
                        | "target"
                        | "dist"
                        | "diagnostics"
                        | "mutants.out"
                )
            )
        })
    })
}

fn text_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("rs" | "toml" | "md" | "sh" | "json" | "yml" | "yaml" | "txt" | "xml")
    )
}

fn physical_lines(path: &Path) -> Result<usize> {
    Ok(fs::read_to_string(path)?.lines().count())
}

fn relative<'a>(root: &Path, path: &'a Path) -> std::borrow::Cow<'a, str> {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy()
}
