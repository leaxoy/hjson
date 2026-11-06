use super::{JsonTree, JsonType};

pub(crate) fn timestamp_hint(doc: &JsonTree, id: usize) -> Option<String> {
    let node = doc.nodes.get(id)?;
    if !matches!(node.typ, JsonType::Number | JsonType::String)
        || !key_suggests_time(node.key.as_deref()?)
    {
        return None;
    }

    let (seconds, nanos) = epoch_parts_from_value(doc.resolve_ref(id)?)?;
    format_unix_utc(seconds, nanos)
}

fn key_suggests_time(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("time")
        || lower.contains("epoch")
        || lower.contains("date")
        || lower.ends_with("_at")
        || lower.ends_with("-at")
        || lower.ends_with("at")
            && [
                "created", "updated", "deleted", "expired", "expires", "started", "ended",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
}

fn epoch_parts_from_value(value: &serde_json::Value) -> Option<(i64, u32)> {
    match value {
        serde_json::Value::Number(number) => epoch_parts_from_number(number),
        serde_json::Value::String(text) => epoch_parts_from_str(text),
        _ => None,
    }
}

fn epoch_parts_from_number(number: &serde_json::Number) -> Option<(i64, u32)> {
    if let Some(value) = number.as_i64() {
        return epoch_parts_from_int(value as i128);
    }
    if let Some(value) = number.as_u64() {
        return epoch_parts_from_int(i128::from(value));
    }
    let value = number.as_f64()?;
    if !value.is_finite() {
        return None;
    }
    epoch_parts_from_f64(value)
}

fn epoch_parts_from_str(text: &str) -> Option<(i64, u32)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse::<i128>().ok().and_then(epoch_parts_from_int);
    }
    if let Some(parts) = epoch_parts_from_decimal_str(text) {
        return Some(parts);
    }
    if text.contains('.') {
        return text.parse::<f64>().ok().and_then(epoch_parts_from_f64);
    }
    None
}

fn epoch_parts_from_decimal_str(text: &str) -> Option<(i64, u32)> {
    let (seconds, fraction) = text.split_once('.')?;
    let unsigned_seconds = seconds.strip_prefix(['+', '-']).unwrap_or(seconds);
    if unsigned_seconds.is_empty()
        || fraction.is_empty()
        || !unsigned_seconds.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }

    let seconds = seconds.parse::<i128>().ok()?;
    if !plausible_epoch_seconds(seconds) {
        return None;
    }
    let mut nanos = 0_u32;
    let mut scale = 100_000_000_u32;
    for byte in fraction.bytes().take(9) {
        nanos += u32::from(byte - b'0') * scale;
        scale /= 10;
    }
    Some((seconds as i64, nanos))
}

fn epoch_parts_from_int(value: i128) -> Option<(i64, u32)> {
    const UNITS: [i128; 4] = [1, 1_000, 1_000_000, 1_000_000_000];
    for unit in UNITS {
        let seconds = value / unit;
        let fraction = value.rem_euclid(unit);
        if plausible_epoch_seconds(seconds) {
            let nanos = ((fraction * 1_000_000_000) / unit) as u32;
            return Some((seconds as i64, nanos));
        }
    }
    None
}

fn epoch_parts_from_f64(value: f64) -> Option<(i64, u32)> {
    if !plausible_epoch_seconds(value as i128) {
        return None;
    }
    let seconds = value.trunc() as i64;
    let nanos = ((value.fract().abs() * 1_000_000_000.0).round() as u32).min(999_999_999);
    Some((seconds, nanos))
}

fn plausible_epoch_seconds(seconds: i128) -> bool {
    // 2000-01-01 through 2100-01-01 keeps ordinary durations like
    // `timeout: 3000` from rendering as timestamps while covering app data.
    (946_684_800..=4_102_444_800).contains(&seconds)
}

fn format_unix_utc(seconds: i64, nanos: u32) -> Option<String> {
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    if nanos == 0 {
        Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
        ))
    } else {
        let millis = nanos / 1_000_000;
        Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{millis:03} UTC"
        ))
    }
}

fn civil_from_days(days_since_epoch: i64) -> Option<(i32, u32, u32)> {
    let z = days_since_epoch.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    Some((i32::try_from(year).ok()?, month as u32, day as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint_for(doc: &JsonTree, key: &str) -> Option<String> {
        let id = doc
            .nodes
            .iter()
            .position(|node| node.key.as_deref() == Some(key))
            .unwrap();
        timestamp_hint(doc, id)
    }

    #[test]
    fn timestamp_hint_formats_epoch_units() {
        let doc = JsonTree::parse(
            r#"{
                "created_time": 1700000000,
                "updated_epoch": 1700000000000,
                "sent_timestamp": 1700000000000000,
                "received_date": 1700000000000000000
            }"#,
        );

        assert_eq!(
            hint_for(&doc, "created_time").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for(&doc, "updated_epoch").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for(&doc, "sent_timestamp").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for(&doc, "received_date").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
    }

    #[test]
    fn timestamp_hint_formats_string_epoch_values() {
        let doc = JsonTree::parse(
            r#"{
                "created_time": "1700000000",
                "updated_epoch": "1700000000000",
                "sent_timestamp": "1700000000.123",
                "bad_time": "not a timestamp"
            }"#,
        );

        assert_eq!(
            hint_for(&doc, "created_time").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for(&doc, "updated_epoch").as_deref(),
            Some("2023-11-14 22:13:20 UTC")
        );
        assert_eq!(
            hint_for(&doc, "sent_timestamp").as_deref(),
            Some("2023-11-14 22:13:20.123 UTC")
        );
        assert_eq!(hint_for(&doc, "bad_time"), None);
    }

    #[test]
    fn timestamp_hint_ignores_non_epoch_durations() {
        let doc = JsonTree::parse(r#"{"timeout": 3000, "count": 1700000000}"#);

        assert_eq!(hint_for(&doc, "timeout"), None);
        assert_eq!(hint_for(&doc, "count"), None);
    }
}
