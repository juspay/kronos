//! Embedded migration templates plus the renderer and apply entry point.

pub mod embedded;
pub mod render;

pub use embedded::{Migration, MIGRATIONS};
pub use render::{render, RenderError};

use crate::schema_config::SchemaConfig;
use sqlx::PgPool;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("template render failed for {migration}: {source}")]
    Render {
        migration: &'static str,
        #[source]
        source: RenderError,
    },
    #[error("SQL execution failed for {migration}: {source}")]
    Sql {
        migration: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

/// Split a SQL script into individual statements, respecting dollar-quoting,
/// single-quoted strings, and `--` line comments. Each returned statement
/// does NOT include the trailing semicolon.
fn split_statements(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut stmts: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i < len {
        match bytes[i] {
            // Line comment: skip to end of line
            b'-' if i + 1 < len && bytes[i + 1] == b'-' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Single-quoted string: skip until matching unescaped quote
            b'\'' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\'' {
                        i += 1;
                        // doubled quote is escape
                        if i < len && bytes[i] == b'\'' {
                            i += 1;
                        } else {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            // Dollar-quoted string: find the tag ($$...$$  or $tag$...$tag$)
            b'$' => {
                // Treat every `$` as the start of a dollar-quote tag. Embedded
                // migrations are DDL and never contain `$1`/`$2` parameter
                // placeholders, so this is safe today. If a future migration
                // body needs raw `$N` placeholders, this branch must learn to
                // distinguish them from dollar-quote tags.
                // Find the closing $ of the opening tag
                let tag_start = i;
                i += 1;
                while i < len && bytes[i] != b'$' {
                    i += 1;
                }
                if i < len {
                    i += 1; // consume closing $
                    let tag = &sql[tag_start..i]; // e.g. "$$" or "$body$"
                    // Now scan for the matching end tag
                    loop {
                        if i + tag.len() > len {
                            // Unterminated; stop scanning
                            i = len;
                            break;
                        }
                        if &sql[i..i + tag.len()] == tag {
                            i += tag.len();
                            break;
                        }
                        i += 1;
                    }
                }
            }
            // Statement terminator
            b';' => {
                let stmt = sql[start..i].trim();
                if !stmt.is_empty() {
                    stmts.push(stmt);
                }
                i += 1;
                start = i;
            }
            _ => {
                i += 1;
            }
        }
    }
    // Trailing content after last semicolon
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        stmts.push(tail);
    }
    stmts
}

/// Render and apply every embedded migration against `pool`.
///
/// Each migration's template is rendered using `cfg`, then each SQL statement
/// is executed individually so that `CREATE INDEX CONCURRENTLY` (which cannot
/// run inside a transaction block) works correctly. The splitter understands
/// dollar-quoted strings and single-quoted strings, so `DO $$ ... $$` blocks
/// are treated as a single statement. Migrations are idempotent (every CREATE
/// uses `IF NOT EXISTS`), so re-running this function on an already-migrated
/// database is safe.
pub async fn apply(pool: &PgPool, cfg: &SchemaConfig) -> Result<(), MigrateError> {
    for m in MIGRATIONS {
        let rendered = render(m.template, cfg).map_err(|e| MigrateError::Render {
            migration: m.name,
            source: e,
        })?;
        tracing::info!(migration = m.name, "applying migration");
        // Split into individual statements and execute each one separately.
        // CREATE INDEX CONCURRENTLY cannot run inside a transaction block;
        // splitting ensures each statement gets its own autocommit connection
        // call rather than being batched into an implicit transaction by
        // PostgreSQL's simple-query protocol.
        for stmt in split_statements(&rendered) {
            let stmt_with_semi = format!("{stmt};");
            sqlx::raw_sql(&stmt_with_semi)
                .execute(pool)
                .await
                .map_err(|e| MigrateError::Sql {
                    migration: m.name,
                    source: e,
                })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::split_statements;

    fn assert_split(input: &str, expected: &[&str]) {
        let got = split_statements(input);
        assert_eq!(got, expected, "input: {input:?}");
    }

    #[test]
    fn empty_input() {
        assert_split("", &[]);
    }

    #[test]
    fn single_with_semicolon() {
        assert_split("SELECT 1;", &["SELECT 1"]);
    }

    #[test]
    fn single_without_semicolon() {
        assert_split("SELECT 1", &["SELECT 1"]);
    }

    #[test]
    fn multiple_statements() {
        assert_split("SELECT 1; SELECT 2;", &["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn semicolon_in_single_quoted_string() {
        assert_split("SELECT 'foo;bar';", &["SELECT 'foo;bar'"]);
    }

    #[test]
    fn escaped_quote_in_string() {
        assert_split("SELECT 'it''s';", &["SELECT 'it''s'"]);
    }

    #[test]
    fn semicolon_in_anonymous_dollar_quote() {
        assert_split("DO $$ BEGIN SELECT 1; END $$;", &["DO $$ BEGIN SELECT 1; END $$"]);
    }

    #[test]
    fn semicolon_in_named_dollar_quote() {
        assert_split(
            "DO $body$ BEGIN SELECT 1; END $body$;",
            &["DO $body$ BEGIN SELECT 1; END $body$"],
        );
    }

    #[test]
    fn line_comment_with_semicolon() {
        assert_split("-- foo;\nSELECT 1;", &["-- foo;\nSELECT 1"]);
    }

    #[test]
    fn do_block_then_create_index() {
        let input = "DO $$ BEGIN PERFORM 1; END $$;\nCREATE INDEX CONCURRENTLY IF NOT EXISTS idx ON t(x);";
        let expected = &[
            "DO $$ BEGIN PERFORM 1; END $$",
            "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx ON t(x)",
        ];
        assert_split(input, expected);
    }
}
