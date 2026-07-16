use std::path::Path;

use chrono::{DateTime, FixedOffset};

use crate::error::BleatError;
use crate::identity::{Role, Slug};
use crate::session::{SessionPath, create_session};

pub fn init(
    root: &Path,
    slug: Slug,
    role: Role,
    created: DateTime<FixedOffset>,
) -> Result<SessionPath, BleatError> {
    create_session(root, slug, role, created)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, FixedOffset};
    use tempfile::tempdir;

    use super::init;
    use crate::error::BleatError;
    use crate::identity::{Role, Slug};
    use crate::session::load_session;

    #[test]
    fn init_registers_the_requested_role() {
        let sandbox = tempdir().expect("sandbox should be created");

        let path = init(
            sandbox.path(),
            Slug::parse("feature").expect("slug should be valid"),
            Role::parse("planner").expect("role should be valid"),
            timestamp("2026-07-16T10:00:00+09:00"),
        )
        .expect("session should initialize");

        let session = load_session(&path.session_json()).expect("session should load");
        let planner = Role::parse("planner").expect("role should be valid");
        assert_eq!(
            session.roles[&planner].registered,
            timestamp("2026-07-16T10:00:00+09:00")
        );
    }

    #[test]
    fn init_rejects_an_existing_session_without_replacing_it() {
        let sandbox = tempdir().expect("sandbox should be created");
        let slug = Slug::parse("feature").expect("slug should be valid");
        let role = Role::parse("planner").expect("role should be valid");
        init(
            sandbox.path(),
            slug.clone(),
            role.clone(),
            timestamp("2026-07-16T10:00:00+09:00"),
        )
        .expect("first init should succeed");

        let error = init(
            sandbox.path(),
            slug,
            role,
            timestamp("2026-07-16T11:00:00+09:00"),
        )
        .expect_err("second init should fail");

        assert!(matches!(error, BleatError::Usage(_)));
    }

    fn timestamp(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).expect("timestamp should be valid")
    }
}
