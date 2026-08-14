use super::*;

#[test]
fn html_is_plain_and_controls_are_removed() {
    let text = plain_text(
        "<html><body><p>Hello <b>world</b></p><img src='https://example.com/a.png'></body></html>\0",
    );
    assert!(text.contains("Hello"));
    assert!(text.contains("world"));
    assert!(!text.contains('<'));
    assert!(!text.contains('\0'));
    assert_eq!(plain_text("one\r\ntwo\rthree\n\tend"), "one\ntwo\nthree\n\tend");
}

#[test]
fn truncation_counts_characters_not_bytes() {
    assert_eq!(truncate("абв", 2), ("аб".into(), true));
    assert_eq!(truncate("short", 10), ("short".into(), false));
}

#[test]
fn numeric_limits_reject_zero_and_overflow() -> anyhow::Result<()> {
    assert_eq!(limit(None, 10, 100)?, 10);
    assert_eq!(limit(Some(100), 10, 100)?, 100);
    assert!(limit(Some(0), 10, 100).is_err());
    assert!(limit(Some(101), 10, 100).is_err());
    Ok(())
}

#[test]
fn filenames_and_mailboxes_are_normalized() {
    assert_eq!(safe_filename(" ../a:b\\c\0 "), "_a_b_c_");
    assert_eq!(safe_filename("..."), "attachment");
    assert_eq!(safe_filename(&"x".repeat(300)).len(), 255);
    assert_eq!(mailbox(" Person <user@example.com> "), "user@example.com");
    assert_eq!(mailbox("user@example.com"), "user@example.com");
    assert_eq!(mailbox("broken<address"), "broken<address");
}
