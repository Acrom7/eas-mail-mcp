#![no_main]

use eas_mail_protocol::CollectionKind;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = eas_mail_protocol::protocol::parse_sync(data, CollectionKind::Mail);
    let _ = eas_mail_protocol::protocol::parse_sync(data, CollectionKind::Calendar);
});
