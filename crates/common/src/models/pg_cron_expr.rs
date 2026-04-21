use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// A validated 5-field cron expression: `minute hour day-of-month month day-of-week`.
/// Matches the format used by standard UNIX cron and pg_cron.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PgCronExpr(String);

impl PgCronExpr {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the 7-field representation required by the `cron` crate
    /// (`sec min hour dom month dow year`) by prepending `0` and appending `*`.
    fn to_seven_field(&self) -> String {
        format!("0 {} *", self.0)
    }

    /// Parse into a `cron::Schedule`. Safe to unwrap because validation
    /// in the constructor already guaranteed parseability.
    pub fn to_schedule(&self) -> cron::Schedule {
        self.to_seven_field()
            .parse()
            .expect("PgCronExpr was validated on construction")
    }
}

impl TryFrom<String> for PgCronExpr {
    type Error = AppError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let field_count = value.split_whitespace().count();
        if field_count != 5 {
            return Err(AppError::InvalidCron(format!(
                "Cron expression must have exactly 5 fields (min hour dom month dow), got {}",
                field_count
            )));
        }

        // Validate by parsing through the `cron` crate (wrapped to 7-field form).
        format!("0 {} *", value)
            .parse::<cron::Schedule>()
            .map_err(|e| AppError::InvalidCron(format!("{}", e)))?;

        Ok(Self(value))
    }
}

impl From<PgCronExpr> for String {
    fn from(expr: PgCronExpr) -> Self {
        expr.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<PgCronExpr, AppError> {
        PgCronExpr::try_from(s.to_string())
    }

    #[test]
    fn accepts_valid_5_field() {
        assert!(parse("* * * * *").is_ok());
        assert!(parse("*/5 * * * *").is_ok());
        assert!(parse("0 9 * * MON-FRI").is_ok());
    }

    #[test]
    fn rejects_7_field() {
        assert!(parse("0 * * * * * *").is_err());
    }

    #[test]
    fn rejects_wrong_field_count() {
        assert!(parse("* * *").is_err());
        assert!(parse("* * * *").is_err());
        assert!(parse("* * * * * *").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not a cron").is_err());
        assert!(parse("99 * * * *").is_err());
    }

    #[test]
    fn round_trips_via_json() {
        let j = serde_json::json!("*/10 * * * *");
        let expr: PgCronExpr = serde_json::from_value(j).unwrap();
        assert_eq!(expr.as_str(), "*/10 * * * *");
        assert_eq!(serde_json::to_value(&expr).unwrap(), "*/10 * * * *");
    }
}
