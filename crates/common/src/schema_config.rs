//! `SchemaConfig` carries the two schema-namespacing parameters that flow
//! through migrations, runtime SQL, and builders.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaConfig {
    pub system_schema: String,
    pub tenant_schema_prefix: String,
}

impl SchemaConfig {
    /// Service-mode default: preserves today's `public.organizations`
    /// and unprefixed tenant schemas.
    pub fn service_default() -> Self {
        Self {
            system_schema: "public".to_string(),
            tenant_schema_prefix: String::new(),
        }
    }

    /// Library-mode default: avoids collisions with host-app tables.
    pub fn library_default() -> Self {
        Self {
            system_schema: "kronos".to_string(),
            tenant_schema_prefix: "kronos_".to_string(),
        }
    }

    /// Validate that both names are safe for use in raw SQL identifiers.
    /// Returns `Err` with a human-readable reason on failure.
    pub fn validate(&self) -> Result<(), String> {
        if !is_valid_pg_identifier(&self.system_schema) {
            return Err(format!(
                "system_schema {:?} must contain only ASCII letters, digits, and underscores, and be 1-63 chars",
                self.system_schema
            ));
        }
        // Empty prefix is allowed; non-empty prefix must be a valid identifier *prefix*
        if !self.tenant_schema_prefix.is_empty()
            && !is_valid_pg_identifier(&self.tenant_schema_prefix)
        {
            return Err(format!(
                "tenant_schema_prefix {:?} must contain only ASCII letters, digits, and underscores",
                self.tenant_schema_prefix
            ));
        }
        Ok(())
    }
}

fn is_valid_pg_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_default_preserves_today() {
        let c = SchemaConfig::service_default();
        assert_eq!(c.system_schema, "public");
        assert_eq!(c.tenant_schema_prefix, "");
        c.validate().expect("service default must validate");
    }

    #[test]
    fn library_default_uses_kronos_namespace() {
        let c = SchemaConfig::library_default();
        assert_eq!(c.system_schema, "kronos");
        assert_eq!(c.tenant_schema_prefix, "kronos_");
        c.validate().expect("library default must validate");
    }

    #[test]
    fn rejects_sql_injection_attempts() {
        let bad = SchemaConfig {
            system_schema: "public; DROP TABLE x;".to_string(),
            tenant_schema_prefix: String::new(),
        };
        let err = bad.validate().unwrap_err();
        assert!(
            err.contains("system_schema"),
            "error should name the offending field, got: {err}"
        );
        assert!(
            err.contains("ASCII") || err.contains("alphanumeric"),
            "error should describe the rule, got: {err}"
        );
    }

    #[test]
    fn rejects_invalid_tenant_prefix() {
        let bad = SchemaConfig {
            system_schema: "public".to_string(),
            tenant_schema_prefix: "bad-prefix!".to_string(),
        };
        let err = bad.validate().unwrap_err();
        assert!(
            err.contains("tenant_schema_prefix"),
            "error should name the offending field, got: {err}"
        );
    }

    #[test]
    fn accepts_63_char_identifier() {
        let name = "a".repeat(63);
        let c = SchemaConfig {
            system_schema: name,
            tenant_schema_prefix: String::new(),
        };
        c.validate().expect("63-char identifier should validate");
    }

    #[test]
    fn rejects_64_char_identifier() {
        let name = "a".repeat(64);
        let c = SchemaConfig {
            system_schema: name,
            tenant_schema_prefix: String::new(),
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_empty_system_schema() {
        let bad = SchemaConfig {
            system_schema: String::new(),
            tenant_schema_prefix: String::new(),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn empty_prefix_is_valid() {
        let c = SchemaConfig {
            system_schema: "public".to_string(),
            tenant_schema_prefix: String::new(),
        };
        c.validate().unwrap();
    }
}
