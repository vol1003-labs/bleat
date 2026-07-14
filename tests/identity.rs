use bleat::error::BleatError;
use bleat::identity::{MessageType, Role, Slug};

#[test]
fn slug_accepts_only_safe_identifiers_up_to_64_bytes() {
    let valid = ["a", "0", "feature-2", &"a".repeat(64)];
    let invalid = [
        "",
        "-feature",
        "Feature",
        "feature_name",
        "..",
        &"a".repeat(65),
    ];

    for value in valid {
        assert!(Slug::parse(value).is_ok(), "`{value}` should be valid");
    }
    for value in invalid {
        assert!(Slug::parse(value).is_err(), "`{value}` should be invalid");
    }
}

#[test]
fn role_and_message_type_require_a_letter_and_at_most_32_bytes() {
    let valid = ["a", "codex", "reviewer-2", &"a".repeat(32)];
    let invalid = [
        "",
        "2codex",
        "-codex",
        "Codex",
        "code_review",
        &"a".repeat(33),
    ];

    for value in valid {
        assert!(Role::parse(value).is_ok(), "role `{value}` should be valid");
        assert!(
            MessageType::parse(value).is_ok(),
            "type `{value}` should be valid"
        );
    }
    for value in invalid {
        assert!(
            Role::parse(value).is_err(),
            "role `{value}` should be invalid"
        );
        assert!(
            MessageType::parse(value).is_err(),
            "type `{value}` should be invalid"
        );
    }
}

#[test]
fn validated_identifiers_serialize_as_plain_strings() {
    let role = Role::parse("codex").expect("fixture should be valid");

    let json = serde_json::to_string(&role).expect("role should serialize");
    let decoded: Role = serde_json::from_str(&json).expect("role should deserialize");

    assert_eq!(json, r#""codex""#);
    assert_eq!(decoded, role);
    assert_eq!(decoded.as_str(), "codex");
}

#[test]
fn errors_map_to_the_documented_exit_codes() {
    assert_eq!(BleatError::Usage("bad input".into()).exit_code(), 2);
    assert_eq!(BleatError::Runtime("I/O failed".into()).exit_code(), 1);
}
