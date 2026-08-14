use eas_mail_protocol::wbxml::{Element, Node, decode, encode};
use proptest::prelude::*;

#[test]
fn str_i_and_opaque_round_trip_across_code_pages() -> eas_mail_protocol::Result<()> {
    let mut root = Element::new("ComposeMail", "SendMail");
    root.push(Element::text("ComposeMail", "ClientId", "client-1"));
    let mut mime = Element::new("ComposeMail", "Mime");
    mime.content.push(Node::Opaque(vec![0, 1, 2, 255]));
    root.push(mime);
    let encoded = encode(&root)?;
    assert!(encoded.windows(6).any(|bytes| bytes == [0xC3, 0x04, 0x00, 0x01, 0x02, 0xFF]));
    assert_eq!(decode(&encoded)?, Some(root));
    Ok(())
}

#[test]
fn malformed_and_unsupported_wbxml_is_rejected() {
    assert!(decode(&[0x03, 0x01, 0x6A, 0x00, 0x45, 0x03, b'x']).is_err());
    assert!(decode(&[0x03, 0x01, 0x6A, 0x00, 0x02]).is_err());
    assert!(decode(&[0x03, 0x01, 0x6A, 0x00, 0xC3, 0x7F]).is_err());
}

#[test]
fn nul_in_inline_text_is_rejected() {
    let root = Element::text("AirSync", "SyncKey", "bad\0text");
    assert!(encode(&root).is_err());
}

#[test]
fn multi_megabyte_document_below_the_limit_round_trips() -> eas_mail_protocol::Result<()> {
    let mut root = Element::new("ComposeMail", "Mime");
    root.content.push(Node::Opaque(vec![0xA5; 2 * 1024 * 1024]));
    let encoded = encode(&root)?;
    assert_eq!(decode(&encoded)?, Some(root));
    Ok(())
}

#[test]
fn nesting_limit_accepts_128_elements_and_rejects_129() -> eas_mail_protocol::Result<()> {
    let accepted = nested_document(128);
    let encoded = encode(&accepted)?;
    assert_eq!(decode(&encoded)?, Some(accepted));
    assert!(encode(&nested_document(129)).is_err());
    Ok(())
}

fn nested_document(depth: usize) -> Element {
    let mut element = Element::text("AirSync", "SyncKey", "1");
    for _ in 1..depth {
        let mut parent = Element::new("AirSync", "Collection");
        parent.push(element);
        element = parent;
    }
    element
}

proptest! {
    #[test]
    fn ascii_inline_strings_round_trip(value in "[ -~]{0,256}") {
        let root = Element::text("Email", "Subject", value);
        let encoded = encode(&root)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        let decoded = decode(&encoded)
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(decoded, Some(root));
    }
}
