use chrono::{DateTime, FixedOffset};

use crate::error::BleatError;
use crate::fs::{SessionLock, atomic_replace};
use crate::identity::Role;
use crate::session::{RoleRecord, SessionPath, encode_session, load_session};

pub fn join(
    session_path: &SessionPath,
    role: Role,
    registered: DateTime<FixedOffset>,
) -> Result<(), BleatError> {
    if !session_path.directory().is_dir() || !session_path.session_json().is_file() {
        return Err(BleatError::Usage(format!(
            "session `{}` does not exist",
            session_path.slug().as_str()
        )));
    }
    let _lock = SessionLock::acquire(&session_path.directory())?;
    let mut session = load_session(&session_path.session_json())?;

    session
        .roles
        .entry(role)
        .and_modify(|record| record.registered = registered)
        .or_insert_with(|| RoleRecord {
            registered,
            cmd: None,
            extra: Default::default(),
        });
    atomic_replace(&session_path.session_json(), &encode_session(&session)?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use chrono::{DateTime, FixedOffset};
    use tempfile::tempdir;

    use super::join;
    use crate::error::BleatError;
    use crate::identity::{Role, Slug};
    use crate::session::{SessionPath, create_session, load_session};

    #[test]
    fn join_registers_a_new_role() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());

        join(&path, role("codex"), timestamp("2026-07-16T11:00:00+09:00"))
            .expect("join should succeed");

        let joined = load_session(&path.session_json()).expect("session should load");
        assert_eq!(
            joined.roles[&role("codex")].registered,
            timestamp("2026-07-16T11:00:00+09:00")
        );
    }

    #[test]
    fn joining_the_same_role_updates_its_registered_timestamp() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());

        join(
            &path,
            role("claude"),
            timestamp("2026-07-16T12:00:00+09:00"),
        )
        .expect("rejoin should succeed");

        let joined = load_session(&path.session_json()).expect("session should load");
        assert_eq!(
            joined.roles[&role("claude")].registered,
            timestamp("2026-07-16T12:00:00+09:00")
        );
    }

    #[test]
    fn joining_a_missing_session_reports_usage() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        fs::remove_dir_all(path.directory()).expect("session should be removed");

        let error = join(&path, role("codex"), timestamp("2026-07-16T11:00:00+09:00"))
            .expect_err("missing session should fail");

        assert!(matches!(error, BleatError::Usage(_)));
    }

    #[test]
    fn joining_preserves_unknown_session_and_role_fields() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = session(sandbox.path());
        let mut value: serde_json::Value = serde_json::from_slice(
            &fs::read(path.session_json()).expect("session should be readable"),
        )
        .expect("session should decode");
        value["future_top_field"] = serde_json::json!(true);
        value["roles"]["claude"]["future_role_field"] = serde_json::json!(7);
        fs::write(
            path.session_json(),
            serde_json::to_vec_pretty(&value).expect("fixture should encode"),
        )
        .expect("fixture should be written");

        join(
            &path,
            role("claude"),
            timestamp("2026-07-16T12:00:00+09:00"),
        )
        .expect("rejoin should succeed");

        let bytes = fs::read(path.session_json()).expect("session should be readable");
        let joined: serde_json::Value =
            serde_json::from_slice(&bytes).expect("session should decode");
        assert_eq!(joined["future_top_field"], true);
        assert_eq!(joined["roles"]["claude"]["future_role_field"], 7);
    }

    fn session(root: &Path) -> SessionPath {
        create_session(
            root,
            Slug::parse("feature").expect("slug should be valid"),
            role("claude"),
            timestamp("2026-07-15T10:00:00+09:00"),
        )
        .expect("session should be created")
    }

    fn role(value: &str) -> Role {
        Role::parse(value).expect("role should be valid")
    }

    fn timestamp(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).expect("timestamp should be valid")
    }
}
