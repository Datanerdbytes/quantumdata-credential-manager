use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedEnv {
    pub entries: Vec<EnvEntry>,
    pub skipped: Vec<String>,
}

pub fn parse(contents: &str) -> ParsedEnv {
    let mut parsed = ParsedEnv::default();
    let mut seen = HashSet::new();

    for (index, raw) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.trim_start_matches('\u{feff}').trim();
        let line = line
            .strip_prefix("export ")
            .map(str::trim_start)
            .unwrap_or(line);

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            parsed
                .skipped
                .push(format!("line {line_number}: missing '='"));
            continue;
        };

        let key = key.trim();
        if !is_valid_key(key) {
            parsed
                .skipped
                .push(format!("line {line_number}: invalid variable name"));
            continue;
        }

        let Some(value) = unquote(value.trim()) else {
            parsed.skipped.push(format!("{key}: unterminated quote"));
            continue;
        };

        if value.is_empty() {
            parsed.skipped.push(format!("{key}: empty value"));
            continue;
        }

        if !seen.insert(key.to_string()) {
            parsed.skipped.push(format!("{key}: duplicate in file"));
            continue;
        }

        parsed.entries.push(EnvEntry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    parsed
}

fn is_valid_key(key: &str) -> bool {
    let mut characters = key.chars();
    let Some(first) = characters.next() else {
        return false;
    };

    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn unquote(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return Some(value);
    };

    let quoted = first == b'"' || first == b'\'';
    if !quoted {
        return Some(value);
    }

    if bytes.len() < 2 || last != first {
        return None;
    }

    Some(&value[1..value.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_export_and_quoted_values() {
        let parsed = parse(
            r#"
            # comment
            OPENAI_API_KEY=sk-plain
            export DATABRICKS_TOKEN='dapi-quoted'
            SLACK_BOT_TOKEN="xoxb-quoted"
            MULTI=part=with=equals
            WITH_SPACE=value with spaces
            "#,
        );

        assert_eq!(
            parsed.entries,
            vec![
                EnvEntry {
                    key: "OPENAI_API_KEY".into(),
                    value: "sk-plain".into()
                },
                EnvEntry {
                    key: "DATABRICKS_TOKEN".into(),
                    value: "dapi-quoted".into()
                },
                EnvEntry {
                    key: "SLACK_BOT_TOKEN".into(),
                    value: "xoxb-quoted".into()
                },
                EnvEntry {
                    key: "MULTI".into(),
                    value: "part=with=equals".into()
                },
                EnvEntry {
                    key: "WITH_SPACE".into(),
                    value: "value with spaces".into()
                },
            ]
        );
        assert!(parsed.skipped.is_empty());
    }

    #[test]
    fn skips_invalid_lines_without_exposing_values() {
        let parsed = parse(
            r#"
            not a variable
            1INVALID=secret-one
            EMPTY=
            QUOTED="unterminated
            TOP_SECRET=secret-two
            TOP_SECRET=secret-three
            "#,
        );

        assert_eq!(
            parsed.entries,
            vec![EnvEntry {
                key: "TOP_SECRET".into(),
                value: "secret-two".into()
            }]
        );
        assert_eq!(parsed.skipped.len(), 5);
        for leaked in ["secret-one", "secret-two", "secret-three"] {
            assert!(parsed.skipped.iter().all(|reason| !reason.contains(leaked)));
        }
        assert!(
            parsed
                .skipped
                .iter()
                .any(|reason| reason.contains("duplicate"))
        );
        assert!(
            parsed
                .skipped
                .iter()
                .any(|reason| reason.contains("unterminated"))
        );
    }

    #[test]
    fn keeps_inline_hashes_as_part_of_the_value() {
        let parsed = parse("TOKEN=abc#123\n");

        assert_eq!(parsed.entries[0].value, "abc#123");
    }

    #[test]
    fn key_validation_accepts_standard_names() {
        assert!(is_valid_key("OPENAI_API_KEY"));
        assert!(is_valid_key("_private"));
        assert!(is_valid_key("lowercase"));
        assert!(!is_valid_key(""));
        assert!(!is_valid_key("1LEADING_DIGIT"));
        assert!(!is_valid_key("WITH-DASH"));
        assert!(!is_valid_key("WITH SPACE"));
    }
}
