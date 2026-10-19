// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Parse a document's frontmatter block and render it as a Markdown table, instead of silently
//! discarding it.

/// A document's frontmatter format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontmatterFormat {
    /// YAML frontmatter, delimited by `---`/`---` or `---`/`...`.
    Yaml,
    /// TOML frontmatter, delimited by `+++`/`+++`.
    Toml,
}

/// Split `input` into the remaining document and its frontmatter block, if present.
///
/// YAML frontmatter is a `---` block at the very start of the input, closed by another `---` or
/// `...` line. TOML frontmatter is a `+++` block closed by another `+++` line. A leading UTF-8
/// byte order mark and trailing whitespace on the delimiter lines are ignored. The returned
/// frontmatter text is everything between the delimiter lines, excluding both delimiters.
///
/// If no valid frontmatter block is found, returns `(input, None)`.
pub fn split_frontmatter(input: &str) -> (&str, Option<(FrontmatterFormat, &str)>) {
    let body = input.strip_prefix('\u{feff}').unwrap_or(input);
    let (first_line, mut rest) = match body.find('\n') {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => return (input, None),
    };
    let (format, closers): (_, &[&str]) = match first_line.trim_end() {
        "---" => (FrontmatterFormat::Yaml, &["---", "..."]),
        "+++" => (FrontmatterFormat::Toml, &["+++"]),
        _ => return (input, None),
    };

    let frontmatter_start = rest;
    while !rest.is_empty() {
        let (line, next) = match rest.find('\n') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, ""),
        };
        if closers.contains(&line.trim_end()) {
            let frontmatter_len = frontmatter_start.len() - rest.len();
            return (next, Some((format, &frontmatter_start[..frontmatter_len])));
        }
        rest = next;
    }

    (input, None)
}

/// Render a frontmatter block as GFM Markdown table source text, one row per top-level key.
///
/// Returns `None` if `raw` fails to parse as `format`, or parses to something other than a
/// mapping at the top level (e.g. a bare scalar or list) -- there is no table to render in that
/// case, and callers should fall back to not rendering anything (equivalent to stripping).
///
/// Nested arrays and mappings are rendered as a compact, comma/colon-joined inline summary
/// rather than as nested tables, since frontmatter is overwhelmingly a flat key-value map in
/// practice and a compact fallback keeps rarer, more complex frontmatter at least readable.
pub fn frontmatter_table_markdown(format: FrontmatterFormat, raw: &str) -> Option<String> {
    let rows: Vec<(String, String)> = match format {
        FrontmatterFormat::Yaml => {
            let value: serde_yaml::Value = serde_yaml::from_str(raw).ok()?;
            let serde_yaml::Value::Mapping(mapping) = value else {
                return None;
            };
            mapping
                .into_iter()
                .map(|(key, value)| (yaml_scalar_to_string(&key), yaml_value_to_string(&value)))
                .collect()
        }
        FrontmatterFormat::Toml => {
            let value: toml::Value = raw.parse().ok()?;
            let toml::Value::Table(table) = value else {
                return None;
            };
            table
                .into_iter()
                .map(|(key, value)| (key, toml_value_to_string(&value)))
                .collect()
        }
    };

    if rows.is_empty() {
        return None;
    }

    let mut markdown = String::from("| Key | Value |\n| --- | --- |\n");
    for (key, value) in rows {
        markdown.push_str("| ");
        markdown.push_str(&escape_table_cell(&key));
        markdown.push_str(" | ");
        markdown.push_str(&escape_table_cell(&value));
        markdown.push_str(" |\n");
    }
    Some(markdown)
}

/// Escape a value for use inside a single GFM table cell: pipes would otherwise end the cell
/// early, and a table row must be a single line, so embedded line breaks become `<br>` (which
/// mdcat's table rendering already turns into an explicit line break within the cell).
fn escape_table_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace("\r\n", "\n")
        .replace('\n', "<br>")
}

fn yaml_scalar_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        other => yaml_value_to_string(other),
    }
}

fn yaml_value_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => String::new(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Sequence(items) => items
            .iter()
            .map(yaml_value_to_string)
            .collect::<Vec<_>>()
            .join(", "),
        serde_yaml::Value::Mapping(mapping) => mapping
            .iter()
            .map(|(k, v)| format!("{}: {}", yaml_scalar_to_string(k), yaml_value_to_string(v)))
            .collect::<Vec<_>>()
            .join(", "),
        serde_yaml::Value::Tagged(tagged) => yaml_value_to_string(&tagged.value),
    }
}

fn toml_value_to_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(items) => items
            .iter()
            .map(toml_value_to_string)
            .collect::<Vec<_>>()
            .join(", "),
        toml::Value::Table(table) => table
            .iter()
            .map(|(k, v)| format!("{k}: {}", toml_value_to_string(v)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_frontmatter_yaml() {
        assert_eq!(
            split_frontmatter("---\na: 1\n---\n# H\n"),
            ("# H\n", Some((FrontmatterFormat::Yaml, "a: 1\n")))
        );
        assert_eq!(
            split_frontmatter("---\r\na: 1\r\n...\r\n# H\n"),
            ("# H\n", Some((FrontmatterFormat::Yaml, "a: 1\r\n")))
        );
    }

    #[test]
    fn split_frontmatter_toml() {
        assert_eq!(
            split_frontmatter("+++\na = 1\n+++\n# H\n"),
            ("# H\n", Some((FrontmatterFormat::Toml, "a = 1\n")))
        );
        assert_eq!(
            split_frontmatter("+++\na = 1\n---\n"),
            ("+++\na = 1\n---\n", None)
        );
    }

    #[test]
    fn split_frontmatter_bom_and_trailing_whitespace() {
        assert_eq!(
            split_frontmatter("\u{feff}---\na: 1\n---\n# H\n").0,
            "# H\n"
        );
        assert_eq!(split_frontmatter("--- \na: 1\n---\t\n# H\n").0, "# H\n");
    }

    #[test]
    fn split_frontmatter_without_closing_delimiter_leaves_input_unchanged() {
        assert_eq!(split_frontmatter("---\na: 1\n"), ("---\na: 1\n", None));
        assert_eq!(split_frontmatter("# H\n---\n"), ("# H\n---\n", None));
        assert_eq!(split_frontmatter("---"), ("---", None));
    }

    #[test]
    fn split_frontmatter_closing_delimiter_at_eof() {
        assert_eq!(
            split_frontmatter("---\na: 1\n---"),
            ("", Some((FrontmatterFormat::Yaml, "a: 1\n")))
        );
    }

    #[test]
    fn frontmatter_table_markdown_yaml_flat_mapping() {
        let markdown =
            frontmatter_table_markdown(FrontmatterFormat::Yaml, "title: My Post\ndraft: false\n")
                .unwrap();
        assert_eq!(
            markdown,
            "| Key | Value |\n| --- | --- |\n| title | My Post |\n| draft | false |\n"
        );
    }

    #[test]
    fn frontmatter_table_markdown_toml_flat_table() {
        let markdown = frontmatter_table_markdown(
            FrontmatterFormat::Toml,
            "title = \"My Post\"\ndraft = false\n",
        )
        .unwrap();
        assert_eq!(
            markdown,
            "| Key | Value |\n| --- | --- |\n| title | My Post |\n| draft | false |\n"
        );
    }

    #[test]
    fn frontmatter_table_markdown_renders_lists_and_nested_maps_compactly() {
        let markdown = frontmatter_table_markdown(
            FrontmatterFormat::Yaml,
            "tags:\n  - rust\n  - cli\nmeta:\n  a: 1\n",
        )
        .unwrap();
        assert_eq!(
            markdown,
            "| Key | Value |\n| --- | --- |\n| tags | rust, cli |\n| meta | a: 1 |\n"
        );
    }

    #[test]
    fn frontmatter_table_markdown_escapes_pipes_and_newlines() {
        let markdown =
            frontmatter_table_markdown(FrontmatterFormat::Yaml, "title: \"a | b\\nsecond line\"\n")
                .unwrap();
        assert_eq!(
            markdown,
            "| Key | Value |\n| --- | --- |\n| title | a \\| b<br>second line |\n"
        );
    }

    #[test]
    fn frontmatter_table_markdown_none_for_non_mapping_top_level() {
        assert_eq!(
            frontmatter_table_markdown(FrontmatterFormat::Yaml, "just a string\n"),
            None
        );
        assert_eq!(
            frontmatter_table_markdown(FrontmatterFormat::Yaml, "- a\n- b\n"),
            None
        );
    }

    #[test]
    fn frontmatter_table_markdown_none_for_unparseable_content() {
        assert_eq!(
            frontmatter_table_markdown(FrontmatterFormat::Toml, "not valid toml :::\n"),
            None
        );
    }

    #[test]
    fn frontmatter_table_markdown_none_for_empty_mapping() {
        assert_eq!(
            frontmatter_table_markdown(FrontmatterFormat::Yaml, "{}\n"),
            None
        );
    }
}
