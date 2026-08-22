use base64::Engine as _;

use crate::{EasError, Result};

const HEADER_LEN: usize = 40;
const INSTANCE_DATE: std::ops::Range<usize> = 16..20;
const VCAL_MARKER: &[u8] = b"vCal-Uid\x01\x00\x00\x00";
const MAX_UID_LENGTH: usize = 300;
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Converts an EAS 14.1 `GlobalObjId` into the corresponding iCalendar UID.
pub fn global_object_id_uid(encoded: &str) -> Result<String> {
    let compact = encoded.chars().filter(|value| !value.is_ascii_whitespace()).collect::<String>();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(compact)
        .map_err(|_| protocol("meeting GlobalObjId is not valid base64"))?;
    let byte_count = decoded
        .get(36..HEADER_LEN)
        .and_then(|value| <[u8; 4]>::try_from(value).ok())
        .map(u32::from_le_bytes)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| protocol("meeting GlobalObjId header is truncated"))?;
    let end = HEADER_LEN
        .checked_add(byte_count)
        .filter(|end| *end <= decoded.len())
        .ok_or_else(|| protocol("meeting GlobalObjId data length is invalid"))?;
    let data = decoded
        .get(HEADER_LEN..end)
        .ok_or_else(|| protocol("meeting GlobalObjId data is missing"))?;
    let uid = if let Some(value) = data.strip_prefix(VCAL_MARKER) {
        let value = value.strip_suffix(&[0]).unwrap_or(value);
        std::str::from_utf8(value)
            .map_err(|_| protocol("meeting GlobalObjId UID is not UTF-8"))?
            .to_owned()
    } else {
        encoded_outlook_uid(&decoded, end)?
    };
    if uid.is_empty() || uid.chars().count() > MAX_UID_LENGTH {
        return Err(protocol("meeting UID length is invalid"));
    }
    Ok(uid)
}

fn encoded_outlook_uid(decoded: &[u8], end: usize) -> Result<String> {
    let mut normalized =
        decoded.get(..end).ok_or_else(|| protocol("meeting GlobalObjId is truncated"))?.to_vec();
    normalized
        .get_mut(INSTANCE_DATE)
        .ok_or_else(|| protocol("meeting GlobalObjId instance date is missing"))?
        .fill(0);
    let mut output = String::with_capacity(normalized.len() * 2);
    for value in normalized {
        let high = HEX
            .get(usize::from(value >> 4))
            .copied()
            .ok_or_else(|| protocol("meeting GlobalObjId hex conversion failed"))?;
        let low = HEX
            .get(usize::from(value & 0x0f))
            .copied()
            .ok_or_else(|| protocol("meeting GlobalObjId hex conversion failed"))?;
        output.push(char::from(high));
        output.push(char::from(low));
    }
    Ok(output)
}

fn protocol(message: &'static str) -> EasError {
    EasError::Protocol(message.into())
}
