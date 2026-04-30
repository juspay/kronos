//! Renders SQL migration templates by substituting `{{system_schema}}` and
//! `{{tenant_schema_prefix}}` placeholders. After rendering, the SQL contains
//! no `{{` sequences — that's a post-condition the renderer enforces.

use crate::schema_config::SchemaConfig;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("invalid SchemaConfig: {0}")]
    InvalidConfig(String),
    #[error("template contains unrecognized placeholders after rendering: {0}")]
    UnrenderedPlaceholder(String),
}

pub fn render(template: &str, cfg: &SchemaConfig) -> Result<String, RenderError> {
    cfg.validate().map_err(RenderError::InvalidConfig)?;

    let rendered = template
        .replace("{{system_schema}}", &cfg.system_schema)
        .replace("{{tenant_schema_prefix}}", &cfg.tenant_schema_prefix);

    // Post-condition: any `{{...}}` left over is an unrecognized placeholder.
    if let Some(start) = rendered.find("{{") {
        let snippet: String = rendered
            .chars()
            .skip(start)
            .take(40)
            .collect();
        return Err(RenderError::UnrenderedPlaceholder(snippet));
    }

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_system_schema_placeholder() {
        let cfg = SchemaConfig::service_default();
        let out = render("CREATE TABLE {{system_schema}}.foo();", &cfg).unwrap();
        assert_eq!(out, "CREATE TABLE public.foo();");
    }

    #[test]
    fn renders_tenant_prefix_placeholder() {
        let cfg = SchemaConfig::library_default();
        let out = render("PREFIX={{tenant_schema_prefix}}", &cfg).unwrap();
        assert_eq!(out, "PREFIX=kronos_");
    }

    #[test]
    fn renders_both_placeholders() {
        let cfg = SchemaConfig::library_default();
        let out = render(
            "SELECT * FROM {{system_schema}}.workspaces WHERE schema_name LIKE '{{tenant_schema_prefix}}%';",
            &cfg,
        )
        .unwrap();
        assert_eq!(
            out,
            "SELECT * FROM kronos.workspaces WHERE schema_name LIKE 'kronos_%';"
        );
    }

    #[test]
    fn empty_prefix_renders_to_empty_string() {
        let cfg = SchemaConfig::service_default();
        let out = render("[{{tenant_schema_prefix}}]", &cfg).unwrap();
        assert_eq!(out, "[]");
    }

    #[test]
    fn rejects_unknown_placeholder() {
        let cfg = SchemaConfig::service_default();
        let err = render("SELECT {{unknown_thing}};", &cfg).unwrap_err();
        assert!(matches!(err, RenderError::UnrenderedPlaceholder(_)));
    }

    #[test]
    fn rejects_invalid_config() {
        let cfg = SchemaConfig {
            system_schema: String::new(),
            tenant_schema_prefix: String::new(),
        };
        let err = render("anything", &cfg).unwrap_err();
        assert!(matches!(err, RenderError::InvalidConfig(_)));
    }

    #[test]
    fn service_default_renders_to_public_schema() {
        let cfg = SchemaConfig::service_default();
        let template = include_str!("../../../../migrations/20260318000000_multi_tenancy.sql");
        let rendered = render(template, &cfg).unwrap();
        assert!(rendered.contains("public.organizations"));
        assert!(rendered.contains("public.workspaces"));
        assert!(!rendered.contains("{{"));
    }

    #[test]
    fn library_default_renders_to_kronos_schema() {
        let cfg = SchemaConfig::library_default();
        let template = include_str!("../../../../migrations/20260318000000_multi_tenancy.sql");
        let rendered = render(template, &cfg).unwrap();
        assert!(rendered.contains("kronos.organizations"));
        assert!(rendered.contains("kronos.workspaces"));
        assert!(!rendered.contains("public.organizations"));
        assert!(!rendered.contains("{{"));
    }
}
