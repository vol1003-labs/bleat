use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::BleatError;
use crate::identity::{Role, Slug};

pub const SESSION_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub version: u32,
    pub slug: Slug,
    pub created: DateTime<FixedOffset>,
    pub store: String,
    pub runtime: String,
    pub roles: BTreeMap<Role, RoleRecord>,
    pub runtime_handles: BTreeMap<Role, Value>,
    pub artifacts: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RoleRecord {
    pub registered: DateTime<FixedOffset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub fn load_session(path: &Path) -> Result<Session, BleatError> {
    let bytes = fs::read(path).map_err(|source| {
        BleatError::Runtime(format!(
            "failed to read session `{}`: {source}",
            path.display()
        ))
    })?;
    let session: Session = serde_json::from_slice(&bytes).map_err(|source| {
        BleatError::Runtime(format!(
            "failed to decode session `{}`: {source}",
            path.display()
        ))
    })?;
    validate_version(session.version)?;
    Ok(session)
}

pub fn encode_session(session: &Session) -> Result<Vec<u8>, BleatError> {
    validate_version(session.version)?;
    let mut bytes = serde_json::to_vec_pretty(session)
        .map_err(|source| BleatError::Runtime(format!("failed to encode session: {source}")))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_version(version: u32) -> Result<(), BleatError> {
    if version == SESSION_VERSION {
        Ok(())
    } else {
        Err(BleatError::Runtime(format!(
            "unsupported session version {version}; expected {SESSION_VERSION}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;
    use tempfile::tempdir;

    use super::*;

    const SESSION_JSON: &str = r#"
    {
      "version": 1,
      "slug": "2026-07-13-feature",
      "created": "2026-07-13T10:00:00+09:00",
      "store": "file",
      "runtime": "herdr",
      "roles": {
        "claude": {
          "registered": "2026-07-13T10:00:00+09:00",
          "future_role_field": { "enabled": true }
        },
        "codex": {
          "registered": "2026-07-13T10:01:00+09:00",
          "cmd": ["codex", "--model", "gpt"]
        }
      },
      "runtime_handles": {
        "claude": { "terminal_id": "term_1", "future_handle": 7 }
      },
      "artifacts": { "spec": ".superpowers/specs/feature.md" },
      "future_top_field": { "enabled": true }
    }
    "#;

    #[test]
    fn session_round_trip_preserves_known_and_unknown_fields() {
        let session: Session = serde_json::from_str(SESSION_JSON).expect("fixture should decode");

        let encoded = encode_session(&session).expect("session should encode");
        let value: Value = serde_json::from_slice(&encoded).expect("encoded session should decode");

        assert_eq!(session.version, 1);
        assert_eq!(session.slug.as_str(), "2026-07-13-feature");
        assert_eq!(session.roles.len(), 2);
        assert_eq!(value["future_top_field"]["enabled"], true);
        assert_eq!(
            value["roles"]["claude"]["future_role_field"]["enabled"],
            true
        );
        assert_eq!(value["runtime_handles"]["claude"]["future_handle"], 7);
        assert_eq!(value["artifacts"]["spec"], ".superpowers/specs/feature.md");
    }

    #[test]
    fn loading_an_unsupported_session_version_fails() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = sandbox.path().join("session.json");
        fs::write(
            &path,
            SESSION_JSON.replace("\"version\": 1", "\"version\": 2"),
        )
        .expect("fixture should be written");

        let error = load_session(&path).expect_err("version 2 should be rejected");

        assert!(matches!(error, crate::error::BleatError::Runtime(_)));
        assert!(error.to_string().contains("unsupported session version 2"));
    }
}
