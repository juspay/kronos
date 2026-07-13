use crate::models::pg_cron_expr::PgCronExpr;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Lenient deserialization for the timestamp fields on job creation/update.
///
/// HTML `<input type="datetime-local">` (used by the dashboard's job form)
/// emits values like `2026-06-09T17:18` — no seconds and no timezone offset.
/// chrono's default `DateTime<Utc>` deserializer only accepts full RFC 3339
/// (`2026-06-09T17:18:00Z`), so those values failed with the opaque
/// "premature end of input" serde error. This accepts both, treating
/// timezone-less values as UTC.
mod flexible_datetime {
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
    use serde::{Deserialize, Deserializer};

    pub fn deserialize_opt<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<String>::deserialize(deserializer)? {
            None => Ok(None),
            Some(s) if s.trim().is_empty() => Ok(None),
            Some(s) => parse(s.trim()).map(Some).map_err(serde::de::Error::custom),
        }
    }

    fn parse(s: &str) -> Result<DateTime<Utc>, String> {
        // Absolute instant carrying an offset or `Z`.
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(dt.with_timezone(&Utc));
        }
        // `datetime-local` wall-clock value, with or without seconds.
        // Interpreted as UTC.
        for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
                return Ok(Utc.from_utc_datetime(&naive));
            }
        }
        Err(format!(
            "invalid datetime '{s}': expected an RFC 3339 timestamp \
             (e.g. 2026-06-09T17:18:00Z) or a datetime-local value \
             (e.g. 2026-06-09T17:18)"
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn accepts_datetime_local_without_seconds() {
            assert_eq!(
                parse("2026-06-09T17:18").unwrap(),
                Utc.with_ymd_and_hms(2026, 6, 9, 17, 18, 0).unwrap()
            );
        }

        #[test]
        fn accepts_datetime_local_with_seconds() {
            assert_eq!(
                parse("2026-06-09T17:18:30").unwrap(),
                Utc.with_ymd_and_hms(2026, 6, 9, 17, 18, 30).unwrap()
            );
        }

        #[test]
        fn accepts_rfc3339_with_offset() {
            assert_eq!(
                parse("2026-06-09T17:18:00+05:30").unwrap(),
                Utc.with_ymd_and_hms(2026, 6, 9, 11, 48, 0).unwrap()
            );
        }

        #[test]
        fn rejects_garbage() {
            assert!(parse("not a date").is_err());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
// TODO 1: Use strum macros and serde to automatically make enums from strings
pub enum TriggerType {
    IMMEDIATE,
    DELAYED,
    CRON,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IMMEDIATE => "IMMEDIATE",
            Self::DELAYED => "DELAYED",
            Self::CRON => "CRON",
        }
    }

    pub fn from_str_val(s: &str) -> Option<Self> {
        match s {
            "IMMEDIATE" => Some(Self::IMMEDIATE),
            "DELAYED" => Some(Self::DELAYED),
            "CRON" => Some(Self::CRON),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    ACTIVE,
    RETIRED,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ACTIVE => "ACTIVE",
            Self::RETIRED => "RETIRED",
        }
    }

    pub fn from_str_val(s: &str) -> Option<Self> {
        match s {
            "ACTIVE" => Some(Self::ACTIVE),
            "RETIRED" => Some(Self::RETIRED),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Job {
    pub job_id: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub trigger_type: String,
    pub status: String,
    pub version: i64,
    pub previous_version_id: Option<String>,
    pub replaced_by_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub input: Option<serde_json::Value>,
    pub run_at: Option<DateTime<Utc>>,
    pub cron_expression: Option<String>,
    pub cron_timezone: Option<String>,
    pub cron_starts_at: Option<DateTime<Utc>>,
    pub cron_ends_at: Option<DateTime<Utc>>,
    pub cron_next_run_at: Option<DateTime<Utc>>,
    pub cron_last_tick_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub retired_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateJob {
    pub endpoint: String,
    pub trigger: String,
    pub idempotency_key: Option<String>,
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub max_attempts: Option<i64>,
    #[serde(default, deserialize_with = "flexible_datetime::deserialize_opt")]
    pub run_at: Option<DateTime<Utc>>,
    pub cron: Option<PgCronExpr>,
    pub timezone: Option<String>,
    #[serde(default, deserialize_with = "flexible_datetime::deserialize_opt")]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "flexible_datetime::deserialize_opt")]
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateJob {
    pub cron: Option<PgCronExpr>,
    pub timezone: Option<String>,
    pub input: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "flexible_datetime::deserialize_opt")]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "flexible_datetime::deserialize_opt")]
    pub ends_at: Option<DateTime<Utc>>,
}
