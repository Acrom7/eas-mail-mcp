use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::common::{BodySource, CommentSource, WriteControl};
use crate::{AppError, ErrorCode, Result};

pub(super) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_bytes(path)?).map_err(|_| invalid("command JSON is invalid"))
}

pub(super) fn read_write_json<T: DeserializeOwned>(
    path: &Path,
    control: &WriteControl,
) -> Result<T> {
    if control.idempotency_key.is_some() {
        return Err(mixed());
    }
    let mut value: Value = serde_json::from_slice(&read_bytes(path)?)
        .map_err(|_| invalid("command JSON is invalid"))?;
    let object = value.as_object_mut().ok_or_else(|| invalid("command JSON must be an object"))?;
    object
        .entry("idempotency_key")
        .or_insert_with(|| Value::String(uuid::Uuid::new_v4().to_string()));
    serde_json::from_value(value).map_err(|_| invalid("command JSON does not match the input"))
}

pub(super) fn idempotency_key(control: &WriteControl) -> String {
    control.idempotency_key.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

pub(super) fn body(source: &BodySource) -> Result<String> {
    optional_body(source).map(Option::unwrap_or_default)
}

pub(super) fn optional_body(source: &BodySource) -> Result<Option<String>> {
    match (&source.body, &source.body_file, source.body_stdin) {
        (Some(value), None, false) => Ok(Some(value.clone())),
        (None, Some(path), false) => read_text(path).map(Some),
        (None, None, true) => read_stdin().map(Some),
        (None, None, false) => Ok(None),
        _ => Err(invalid("select only one body input")),
    }
}

pub(super) fn comment(source: &CommentSource) -> Result<String> {
    match (&source.comment, &source.comment_file, source.comment_stdin) {
        (Some(value), None, false) => Ok(value.clone()),
        (None, Some(path), false) => read_text(path),
        (None, None, true) => read_stdin(),
        (None, None, false) => Ok(String::new()),
        _ => Err(invalid("select only one comment input")),
    }
}

pub(super) fn required(value: Option<String>, label: &'static str) -> Result<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(format!("{label} is required unless --input is used")))
}

pub(super) fn selected(values: Vec<String>) -> Option<Vec<String>> {
    (!values.is_empty()).then_some(values)
}

pub(super) fn ensure_flag_mode(path: Option<&PathBuf>, has_flags: bool) -> Result<()> {
    if path.is_some() && has_flags { Err(mixed()) } else { Ok(()) }
}

pub(super) fn invalid(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

fn mixed() -> AppError {
    invalid("--input cannot be combined with command data flags")
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    if path.as_os_str() == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("cannot read command JSON from stdin"))?;
        Ok(bytes)
    } else {
        std::fs::read(path).map_err(|_| invalid("cannot read command JSON file"))
    }
}

fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|_| invalid("cannot read plain-text input file"))
}

fn read_stdin() -> Result<String> {
    let mut value = String::new();
    std::io::stdin()
        .lock()
        .read_to_string(&mut value)
        .map_err(|_| invalid("cannot read plain-text input from stdin"))?;
    Ok(value)
}
