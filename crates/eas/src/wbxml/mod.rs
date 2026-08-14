//! Strict MS-ASWBXML encoder and decoder.

mod codec;
mod codepage;
mod element;

pub use codec::{decode, encode};
pub use codepage::{CodePage, code_page, code_page_for_namespace};
pub use element::{Element, Node};
