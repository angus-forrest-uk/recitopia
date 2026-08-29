use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{SecondsFormat, Utc};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[must_use]
pub fn now_iso8601() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[must_use]
pub fn generate_id(prefix: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{timestamp:x}{counter:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_prefixed_and_unique() {
        let first = generate_id("pantry");
        let second = generate_id("pantry");

        assert!(first.starts_with("pantry-"));
        assert_ne!(first, second);
    }

    #[test]
    fn timestamps_are_utc_rfc3339_values() {
        let timestamp = now_iso8601();

        assert!(timestamp.ends_with('Z'));
        assert!(chrono::DateTime::parse_from_rfc3339(&timestamp).is_ok());
    }
}
