use crate::{AppError, ErrorCode, Result};

pub(super) fn plain_text(value: &str) -> String {
    let converted = if looks_like_html(value) {
        html2text::from_read(value.as_bytes(), 120).unwrap_or_else(|_| value.to_owned())
    } else {
        value.to_owned()
    };
    normalize_controls(&converted)
}

pub(super) fn truncate(value: &str, limit: usize) -> (String, bool) {
    let mut characters = value.chars();
    let output = characters.by_ref().take(limit).collect::<String>();
    let truncated = characters.next().is_some();
    (output, truncated)
}

pub(super) fn limit(value: Option<u32>, default: usize, maximum: usize) -> Result<usize> {
    let value = value.map_or(default, |number| number as usize);
    if value == 0 || value > maximum {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            format!("limit must be between 1 and {maximum}"),
        ));
    }
    Ok(value)
}

pub(super) fn safe_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '_',
            other if other.is_control() => '_',
            other => other,
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches(['.', ' ']);
    if trimmed.is_empty() { "attachment".into() } else { trimmed.chars().take(255).collect() }
}

pub(super) fn mailbox(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(start) = trimmed.rfind('<')
        && trimmed.ends_with('>')
    {
        return trimmed.get(start + 1..trimmed.len() - 1).unwrap_or(trimmed).trim().to_owned();
    }
    trimmed.to_owned()
}

fn looks_like_html(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("<html")
        || lower.contains("<body")
        || lower.contains("<div")
        || lower.contains("<p")
        || lower.contains("<br")
}

fn normalize_controls(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect::<String>()
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

#[cfg(test)]
mod tests;
