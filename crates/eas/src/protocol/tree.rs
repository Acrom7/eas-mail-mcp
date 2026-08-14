use chrono::{DateTime, NaiveDateTime, Utc};

use crate::wbxml::{Element, Node};

pub(super) fn element(namespace: &str, name: &str) -> Element {
    Element::new(namespace, name)
}

pub(super) fn text_element(namespace: &str, name: &str, value: impl Into<String>) -> Element {
    Element::text(namespace, name, value)
}

pub(super) fn push_text(
    parent: &mut Element,
    namespace: &str,
    name: &str,
    value: impl Into<String>,
) {
    parent.push(text_element(namespace, name, value));
}

pub(super) fn direct_text(parent: &Element, namespace: &str, name: &str) -> Option<String> {
    parent.child(namespace, name).map(Element::text_content)
}

pub(super) fn descendant_text(parent: &Element, namespace: &str, name: &str) -> Option<String> {
    parent.descendant(namespace, name).map(Element::text_content)
}

pub(super) fn integer(value: Option<String>, default: u16) -> u16 {
    value.and_then(|value| value.parse().ok()).unwrap_or(default)
}

pub(super) fn parse_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
    let value = value?;
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%SZ").map(|value| value.and_utc())
        })
        .ok()
}

pub(super) fn opaque_element(namespace: &str, name: &str, value: Vec<u8>) -> Element {
    let mut output = element(namespace, name);
    output.content.push(Node::Opaque(value));
    output
}
