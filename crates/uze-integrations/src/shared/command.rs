//! Shared canonical-command reading for integrations that cannot deliver
//! the canonical `commands/<name>.md` bytes as-is and must translate them
//! into a vendor representation (Codex's explicit-only SKILL.md,
//! Antigravity's adapted SKILL.md).
//!
//! The canonical v0 command model (ADR-025) is deliberately minimal: an
//! optional YAML-style frontmatter block whose only consumed field is
//! `description`, followed by the prompt body. Parsing here is best-effort
//! and never fatal — a body that does not parse as expected frontmatter is
//! treated as body-only, which is always deliverable. The canonical bytes
//! themselves remain the Store's payload; this module only reads them.

fn is_utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// Splits a canonical command file into `(description, body)`:
///
/// - A leading `---` line, then key-value lines, then a closing `---` line
///   is frontmatter; only the `description` key is consumed, everything
///   else in the block is deliberately ignored (never reinterpreted, never
///   dropped from the canonical bytes — this function only *reads*).
///   Frontmatter that is malformed falls through to "body-only".
/// - Everything after the closing marker is the body, with surrounding
///   whitespace preserved exactly as shipped (no trim, no rewrite).
pub fn parse_command_body(bytes: &[u8]) -> (Option<String>, String) {
    let Some(text) = is_utf8(bytes) else {
        return (None, String::new());
    };
    let stripped = text.strip_prefix("\u{feff}").unwrap_or(text);
    let Some(rest) = stripped.strip_prefix("---\n") else {
        return (None, text.to_owned());
    };
    let Some(end) = rest.find("\n---\n") else {
        return (None, text.to_owned());
    };
    let head = &rest[..end];
    let body = &rest[end + "\n---\n".len()..];
    let mut description = None;
    for line in head.lines() {
        if let Some((key, value)) = line.split_once(':')
            && key.trim() == "description"
        {
            description = Some(value.trim().to_owned());
        }
    }
    (description, body.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_description_and_preserves_body_verbatim() {
        let bytes = b"---\ndescription: Review code\nothers: ignored\n---\n\nBody line one.\nBody line two.\n";
        let (description, body) = parse_command_body(bytes);
        assert_eq!(description.as_deref(), Some("Review code"));
        assert_eq!(body, "\nBody line one.\nBody line two.\n");
    }

    #[test]
    fn body_only_file_has_no_description() {
        let bytes = b"Just a prompt body.\n";
        let (description, body) = parse_command_body(bytes);
        assert_eq!(description, None);
        assert_eq!(body, "Just a prompt body.\n");
    }

    #[test]
    fn malformed_frontmatter_falls_through_to_body_only() {
        let bytes = b"---\ndescription: broken\nno closing marker\n";
        let (description, body) = parse_command_body(bytes);
        assert_eq!(description, None);
        assert_eq!(body, std::str::from_utf8(bytes).unwrap());
    }

    #[test]
    fn non_utf8_payload_degrades_to_empty_body() {
        let bytes = b"\xff\xfe\x00";
        let (description, body) = parse_command_body(bytes);
        assert_eq!(description, None);
        assert_eq!(body, "");
    }
}
