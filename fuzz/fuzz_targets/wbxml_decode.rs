#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = eas_mail_protocol::wbxml::decode(data);
});
