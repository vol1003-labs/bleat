use serde::{Deserialize, Serialize};

use crate::error::BleatError;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Slug(String);

impl Slug {
    pub fn parse(value: &str) -> Result<Self, BleatError> {
        validate(value, 64, true, "slug")?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Role(String);

impl Role {
    pub fn parse(value: &str) -> Result<Self, BleatError> {
        validate(value, 32, false, "role")?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MessageType(String);

impl MessageType {
    pub fn parse(value: &str) -> Result<Self, BleatError> {
        validate(value, 32, false, "message type")?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate(
    value: &str,
    maximum_length: usize,
    digit_may_start: bool,
    kind: &str,
) -> Result<(), BleatError> {
    let Some((&first, rest)) = value.as_bytes().split_first() else {
        return Err(invalid(kind, value));
    };
    let valid_first = first.is_ascii_lowercase() || (digit_may_start && first.is_ascii_digit());
    let valid_rest = rest
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');

    if value.len() <= maximum_length && valid_first && valid_rest {
        Ok(())
    } else {
        Err(invalid(kind, value))
    }
}

fn invalid(kind: &str, value: &str) -> BleatError {
    BleatError::Usage(format!("invalid {kind}: `{value}`"))
}

macro_rules! impl_string_conversion {
    ($identifier:ty) => {
        impl TryFrom<String> for $identifier {
            type Error = BleatError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$identifier> for String {
            fn from(value: $identifier) -> Self {
                value.0
            }
        }
    };
}

impl_string_conversion!(Slug);
impl_string_conversion!(Role);
impl_string_conversion!(MessageType);

#[cfg(test)]
mod tests {
    use super::*;

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
    fn role_accepts_only_safe_identifiers_up_to_32_bytes() {
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
            assert!(Role::parse(value).is_ok(), "`{value}` should be valid");
        }
        for value in invalid {
            assert!(Role::parse(value).is_err(), "`{value}` should be invalid");
        }
    }

    #[test]
    fn message_type_accepts_only_safe_identifiers_up_to_32_bytes() {
        let valid = ["a", "question", "custom-type", &"a".repeat(32)];
        let invalid = [
            "",
            "2question",
            "-question",
            "Question",
            "custom_type",
            &"a".repeat(33),
        ];

        for value in valid {
            assert!(
                MessageType::parse(value).is_ok(),
                "`{value}` should be valid"
            );
        }
        for value in invalid {
            assert!(
                MessageType::parse(value).is_err(),
                "`{value}` should be invalid"
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
}
