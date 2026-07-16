use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, SecondsFormat};

use crate::error::BleatError;
use crate::identity::{MessageType, Role};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    pub id: u64,
    pub from: Role,
    pub to: Role,
    pub kind: MessageType,
    pub reply_to: Option<u64>,
    pub timestamp: DateTime<FixedOffset>,
    pub body: String,
}

pub fn encode(message: &Message) -> String {
    let reply = message
        .reply_to
        .map(|id| format!("re: {id:04}\n"))
        .unwrap_or_default();
    format!(
        "---\nid: {:04}\nfrom: {}\nto: {}\ntype: {}\n{}ts: {}\n---\n{}",
        message.id,
        message.from.as_str(),
        message.to.as_str(),
        message.kind.as_str(),
        reply,
        message.timestamp.to_rfc3339_opts(SecondsFormat::Secs, true),
        message.body,
    )
}

pub fn decode(expected_id: u64, source: &str) -> Result<Message, BleatError> {
    let contents = source
        .strip_prefix("---\n")
        .and_then(|contents| contents.split_once("\n---\n"))
        .ok_or_else(|| decode_error("missing frontmatter delimiters"))?;
    let fields = parse_fields(contents.0)?;
    let id = parse_id(required_field(&fields, "id")?, "id")?;
    if id != expected_id {
        return Err(decode_error(&format!(
            "expected id {expected_id}, found {id}"
        )));
    }
    let from =
        Role::parse(required_field(&fields, "from")?).map_err(|_| decode_error("invalid from"))?;
    let to = Role::parse(required_field(&fields, "to")?).map_err(|_| decode_error("invalid to"))?;
    let kind = MessageType::parse(required_field(&fields, "type")?)
        .map_err(|_| decode_error("invalid type"))?;
    let reply_to = fields
        .get("re")
        .copied()
        .map(|value| parse_id(value, "reply id"))
        .transpose()?;
    let timestamp = DateTime::parse_from_rfc3339(required_field(&fields, "ts")?)
        .map_err(|_| decode_error("invalid ts"))?;

    Ok(Message {
        id,
        from,
        to,
        kind,
        reply_to,
        timestamp,
        body: contents.1.to_owned(),
    })
}

fn parse_fields(header: &str) -> Result<BTreeMap<&str, &str>, BleatError> {
    let mut fields = BTreeMap::new();
    for line in header.lines() {
        let (name, value) = line
            .split_once(": ")
            .ok_or_else(|| decode_error("invalid frontmatter field"))?;
        if !matches!(name, "id" | "from" | "to" | "type" | "re" | "ts") {
            return Err(decode_error(&format!("unknown field {name}")));
        }
        if fields.insert(name, value).is_some() {
            return Err(decode_error(&format!("duplicate field {name}")));
        }
    }
    Ok(fields)
}

fn parse_id(value: &str, name: &str) -> Result<u64, BleatError> {
    let id = value
        .parse()
        .map_err(|_| decode_error(&format!("invalid {name}")))?;
    if id == 0 || format!("{id:04}") != value {
        Err(decode_error(&format!("invalid {name}")))
    } else {
        Ok(id)
    }
}

fn required_field<'a>(
    fields: &BTreeMap<&'a str, &'a str>,
    name: &str,
) -> Result<&'a str, BleatError> {
    fields
        .get(name)
        .copied()
        .ok_or_else(|| decode_error(&format!("missing {name}")))
}

fn decode_error(detail: &str) -> BleatError {
    BleatError::Execution(format!("failed to decode message: {detail}"))
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use crate::identity::{MessageType, Role};

    use super::*;

    #[test]
    fn encode_formats_a_message_without_a_reply() {
        let message = Message {
            id: 2,
            from: Role::parse("codex").expect("from role should be valid"),
            to: Role::parse("claude").expect("to role should be valid"),
            kind: MessageType::parse("question").expect("message type should be valid"),
            reply_to: None,
            timestamp: DateTime::parse_from_rfc3339("2026-07-13T10:05:00+09:00")
                .expect("timestamp should be valid"),
            body: "Can you review this?".to_owned(),
        };

        let encoded = encode(&message);

        assert_eq!(
            encoded,
            "---\nid: 0002\nfrom: codex\nto: claude\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nCan you review this?"
        );
    }

    #[test]
    fn encode_includes_a_zero_padded_reply_id() {
        let message = Message {
            id: 2,
            from: Role::parse("codex").expect("from role should be valid"),
            to: Role::parse("claude").expect("to role should be valid"),
            kind: MessageType::parse("answer").expect("message type should be valid"),
            reply_to: Some(1),
            timestamp: DateTime::parse_from_rfc3339("2026-07-13T10:05:00+09:00")
                .expect("timestamp should be valid"),
            body: "Reviewed.".to_owned(),
        };

        let encoded = encode(&message);

        assert!(encoded.contains("\nre: 0001\n"));
    }

    #[test]
    fn decode_parses_a_message_without_a_reply() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nCan you review this?";

        let message = decode(2, source).expect("message should decode");

        assert_eq!(
            message,
            Message {
                id: 2,
                from: Role::parse("codex").expect("from role should be valid"),
                to: Role::parse("claude").expect("to role should be valid"),
                kind: MessageType::parse("question").expect("message type should be valid"),
                reply_to: None,
                timestamp: DateTime::parse_from_rfc3339("2026-07-13T10:05:00+09:00")
                    .expect("timestamp should be valid"),
                body: "Can you review this?".to_owned(),
            }
        );
    }

    #[test]
    fn decode_parses_a_reply_id() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: answer\nre: 0001\nts: 2026-07-13T10:05:00+09:00\n---\nReviewed.";

        let message = decode(2, source).expect("message should decode");

        assert_eq!(message.reply_to, Some(1));
    }

    #[test]
    fn decode_preserves_an_empty_body() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: kick\nts: 2026-07-13T10:05:00+09:00\n---\n";

        let message = decode(2, source).expect("empty message should decode");

        assert_eq!(message.body, "");
    }

    #[test]
    fn decode_preserves_delimiters_in_the_body() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: report\nts: 2026-07-13T10:05:00+09:00\n---\nfirst\n---\nsecond";

        let message = decode(2, source).expect("message should decode");

        assert_eq!(message.body, "first\n---\nsecond");
    }

    #[test]
    fn decode_rejects_an_invalid_field_value() {
        let source = "---\nid: 0002\nfrom: Codex\nto: claude\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("invalid from should fail");

        assert!(matches!(error, BleatError::Execution(_)));
        assert!(error.to_string().contains("invalid from"));
    }

    #[test]
    fn decode_rejects_an_unknown_field() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: question\nsubject: review\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("unknown field should fail");

        assert!(error.to_string().contains("unknown field subject"));
    }

    #[test]
    fn decode_rejects_a_missing_field() {
        let source =
            "---\nid: 0002\nfrom: codex\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("missing to should fail");

        assert!(error.to_string().contains("missing to"));
    }

    #[test]
    fn decode_rejects_a_duplicate_field() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\nto: reviewer\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("duplicate to should fail");

        assert!(error.to_string().contains("duplicate field to"));
    }

    #[test]
    fn decode_rejects_an_id_that_differs_from_the_filename() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(3, source).expect_err("mismatched id should fail");

        assert!(error.to_string().contains("expected id 3, found 2"));
    }

    #[test]
    fn decode_rejects_an_unpadded_message_id() {
        let source = "---\nid: 2\nfrom: codex\nto: claude\ntype: question\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("unpadded id should fail");

        assert!(error.to_string().contains("invalid id"));
    }

    #[test]
    fn decode_rejects_an_unpadded_reply_id() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: answer\nre: 1\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("unpadded reply id should fail");

        assert!(error.to_string().contains("invalid reply id"));
    }

    #[test]
    fn decode_rejects_a_zero_reply_id() {
        let source = "---\nid: 0002\nfrom: codex\nto: claude\ntype: answer\nre: 0000\nts: 2026-07-13T10:05:00+09:00\n---\nbody";

        let error = decode(2, source).expect_err("zero reply id should fail");

        assert!(error.to_string().contains("invalid reply id"));
    }
}
