//! TOON tabular format serialization.
//!
//! Produces compact tabular arrays like:
//! ```text
//! # FILE INDEX
//! files[342]{path,type,domain,subdomain,claude_md}:
//! apps/web/src/routes/+page.svelte,component,web,routes,apps/web/CLAUDE.md
//! ```

/// Characters that require quoting in TOON values.
fn needs_quoting(val: &str, delimiter: char) -> bool {
    if val.is_empty() {
        return true;
    }
    val.contains(delimiter)
        || val.contains(':')
        || val.contains('"')
        || val.contains('\n')
        || val.contains('\r')
        || val.contains('\t')
        || val.contains('[')
        || val.contains(']')
        || val.contains('{')
        || val.contains('}')
        || val.starts_with(' ')
        || val.ends_with(' ')
        || val.starts_with('-')
        || val == "true"
        || val == "false"
        || val == "null"
}

/// Escape and optionally quote a TOON value.
pub fn escape_value(val: &str) -> String {
    if val.is_empty() {
        return r#""""#.to_string();
    }

    if needs_quoting(val, ',') {
        let escaped = val
            .replace('\\', r"\\")
            .replace('"', r#"\""#)
            .replace('\n', r"\n")
            .replace('\r', r"\r")
            .replace('\t', r"\t");
        format!("\"{escaped}\"")
    } else {
        val.to_string()
    }
}

/// Render a TOON tabular array from rows (Vec of field-value maps).
///
/// Each row is a slice of string values in the same order as `fields`.
pub fn render_tabular(
    array_name: &str,
    fields: &[&str],
    rows: &[Vec<&str>],
    comment: Option<&str>,
) -> String {
    let mut lines = Vec::with_capacity(rows.len() + 3);

    if let Some(c) = comment {
        lines.push(format!("# {c}"));
    }

    let field_header = fields.join(",");
    lines.push(format!("{array_name}[{}]{{{field_header}}}:", rows.len()));

    for row in rows {
        let escaped: Vec<String> = row.iter().map(|v| escape_value(v)).collect();
        lines.push(escaped.join(","));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_simple() {
        assert_eq!(escape_value("hello"), "hello");
    }

    #[test]
    fn test_escape_empty() {
        assert_eq!(escape_value(""), r#""""#);
    }

    #[test]
    fn test_escape_comma() {
        assert_eq!(escape_value("a,b"), r#""a,b""#);
    }

    #[test]
    fn test_escape_quotes() {
        assert_eq!(escape_value(r#"say "hi""#), r#""say \"hi\"""#);
    }

    #[test]
    fn test_render_tabular() {
        let rows = vec![
            vec!["src/main.rs", "module", "root"],
            vec!["src/lib.rs", "module", "root"],
        ];
        let result = render_tabular("files", &["path", "type", "domain"], &rows, Some("FILE INDEX"));
        assert!(result.contains("files[2]{path,type,domain}:"));
        assert!(result.contains("src/main.rs,module,root"));
    }
}
