use crate::{EasError, Result};

/// A generated MS-ASWBXML code page.
#[derive(Debug)]
pub struct CodePage {
    /// Numeric page identifier.
    pub id: u8,
    /// EAS XML namespace used on the wire.
    pub namespace: &'static str,
    /// Human-friendly namespace alias.
    pub xmlns: &'static str,
    /// Token-to-tag mapping.
    pub tags: &'static [(u8, &'static str)],
}

include!(concat!(env!("OUT_DIR"), "/codepages.rs"));

/// Looks up a code page by numeric identifier.
pub fn code_page(id: u8) -> Result<&'static CodePage> {
    CODE_PAGES
        .get(usize::from(id))
        .filter(|page| page.id == id)
        .ok_or_else(|| EasError::Protocol(format!("unknown WBXML code page 0x{id:02X}")))
}

/// Looks up a code page by EAS namespace.
pub fn code_page_for_namespace(namespace: &str) -> Result<&'static CodePage> {
    CODE_PAGES
        .iter()
        .find(|page| page.namespace.eq_ignore_ascii_case(namespace))
        .ok_or_else(|| EasError::Protocol(format!("unknown EAS namespace {namespace:?}")))
}

impl CodePage {
    pub(crate) fn tag(&self, token: u8) -> Result<&'static str> {
        self.tags
            .iter()
            .find_map(|(candidate, name)| (*candidate == token).then_some(*name))
            .ok_or_else(|| {
                EasError::Protocol(format!("unknown token 0x{token:02X} on WBXML page {}", self.id))
            })
    }

    pub(crate) fn token(&self, name: &str) -> Result<u8> {
        self.tags
            .iter()
            .find_map(|(token, candidate)| (*candidate == name).then_some(*token))
            .ok_or_else(|| {
                EasError::Protocol(format!(
                    "unknown tag {name:?} in EAS namespace {:?}",
                    self.namespace
                ))
            })
    }
}
