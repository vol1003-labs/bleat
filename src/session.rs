use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::BleatError;
use crate::fs::atomic_replace;
use crate::identity::{Role, Slug};
use crate::runtime::RuntimeHandle;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPath {
    root: PathBuf,
    slug: Slug,
}

impl SessionPath {
    fn new(root: &Path, slug: Slug) -> Self {
        Self {
            root: root.to_path_buf(),
            slug,
        }
    }

    pub fn slug(&self) -> &Slug {
        &self.slug
    }

    pub fn directory(&self) -> PathBuf {
        self.root.join(".bleat").join(self.slug.as_str())
    }

    pub fn session_json(&self) -> PathBuf {
        self.directory().join("session.json")
    }

    pub fn messages_dir(&self) -> PathBuf {
        self.directory().join("messages")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionResolution {
    pub path: SessionPath,
    pub warning: Option<String>,
}

pub fn create_session(
    root: &Path,
    slug: Slug,
    role: Role,
    created: DateTime<FixedOffset>,
) -> Result<SessionPath, BleatError> {
    create_session_with_runtime_handle(root, slug, role, created, None)
}

pub(crate) fn create_session_with_handle(
    root: &Path,
    slug: Slug,
    role: Role,
    created: DateTime<FixedOffset>,
    handle: RuntimeHandle,
) -> Result<SessionPath, BleatError> {
    create_session_with_runtime_handle(root, slug, role, created, Some(handle))
}

fn create_session_with_runtime_handle(
    root: &Path,
    slug: Slug,
    role: Role,
    created: DateTime<FixedOffset>,
    handle: Option<RuntimeHandle>,
) -> Result<SessionPath, BleatError> {
    let bleat_root = root.join(".bleat");
    fs::create_dir_all(&bleat_root)
        .map_err(|source| session_io_error("create bleat directory", &bleat_root, source))?;
    let path = SessionPath::new(root, slug.clone());
    let session_dir = path.directory();
    match fs::create_dir(&session_dir) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Err(BleatError::Usage(format!(
                "session `{}` already exists",
                slug.as_str()
            )));
        }
        Err(source) => {
            return Err(session_io_error(
                "create session directory",
                &session_dir,
                source,
            ));
        }
    }

    let result = initialize_session(&path, slug, role, created, handle);
    if result.is_err() {
        let _ = fs::remove_dir_all(&session_dir);
    }
    result.map(|()| path)
}

fn initialize_session(
    path: &SessionPath,
    slug: Slug,
    role: Role,
    created: DateTime<FixedOffset>,
    handle: Option<RuntimeHandle>,
) -> Result<(), BleatError> {
    fs::create_dir(path.messages_dir()).map_err(|source| {
        session_io_error("create messages directory", &path.messages_dir(), source)
    })?;
    let runtime_handles = handle
        .map(|handle| BTreeMap::from([(role.clone(), runtime_handle_value(handle))]))
        .unwrap_or_default();
    let mut roles = BTreeMap::new();
    roles.insert(
        role,
        RoleRecord {
            registered: created,
            cmd: None,
            extra: BTreeMap::new(),
        },
    );
    let session = Session {
        version: SESSION_VERSION,
        slug,
        created,
        store: "file".to_owned(),
        runtime: "herdr".to_owned(),
        roles,
        runtime_handles,
        artifacts: serde_json::json!({}),
        extra: BTreeMap::new(),
    };
    atomic_replace(&path.session_json(), &encode_session(&session)?)
}

pub(crate) fn runtime_handle_value(handle: RuntimeHandle) -> Value {
    let RuntimeHandle {
        terminal_id,
        pane_id,
        agent_name,
    } = handle;
    let mut value = serde_json::Map::from_iter([
        ("terminal_id".to_owned(), Value::String(terminal_id)),
        ("pane_id".to_owned(), Value::String(pane_id)),
    ]);
    if let Some(agent_name) = agent_name {
        value.insert("agent_name".to_owned(), Value::String(agent_name));
    }
    Value::Object(value)
}

pub fn resolve_session(
    root: &Path,
    flag: Option<&str>,
    environment: Option<&str>,
) -> Result<SessionResolution, BleatError> {
    if let Some(selected) = flag.or(environment) {
        return resolve_named_session(root, selected);
    }

    let bleat_root = root.join(".bleat");
    let entries = fs::read_dir(&bleat_root)
        .map_err(|source| session_io_error("read bleat directory", &bleat_root, source))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| {
            session_io_error("read bleat directory entry", &bleat_root, source)
        })?;
        if !entry
            .file_type()
            .map_err(|source| session_io_error("read session file type", &entry.path(), source))?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().into_string().map_err(|_| {
            BleatError::Runtime(format!(
                "session directory name is not UTF-8: `{}`",
                entry.path().display()
            ))
        })?;
        let slug = Slug::parse(&name).map_err(|_| {
            BleatError::Runtime(format!(
                "invalid session directory `{}`",
                entry.path().display()
            ))
        })?;
        let path = SessionPath::new(root, slug);
        let session = load_session(&path.session_json())?;
        ensure_matching_slug(&path, &session)?;
        candidates.push((path, session.created));
    }
    if candidates.is_empty() {
        return Err(BleatError::Usage("no bleat sessions found".to_owned()));
    }
    candidates.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.slug.cmp(&right.0.slug))
    });
    let multiple = candidates.len() > 1;
    let (path, _) = candidates.into_iter().next().ok_or_else(|| {
        BleatError::Runtime("session candidates disappeared during resolution".to_owned())
    })?;
    let warning = multiple.then(|| {
        format!(
            "multiple sessions found; using latest `{}`",
            path.slug.as_str()
        )
    });
    Ok(SessionResolution { path, warning })
}

fn resolve_named_session(root: &Path, selected: &str) -> Result<SessionResolution, BleatError> {
    let path = SessionPath::new(root, Slug::parse(selected)?);
    if !path.session_json().is_file() {
        return Err(BleatError::Usage(format!(
            "session `{selected}` does not exist"
        )));
    }
    let session = load_session(&path.session_json())?;
    ensure_matching_slug(&path, &session)?;
    Ok(SessionResolution {
        path,
        warning: None,
    })
}

fn ensure_matching_slug(path: &SessionPath, session: &Session) -> Result<(), BleatError> {
    if path.slug == session.slug {
        Ok(())
    } else {
        Err(BleatError::Runtime(format!(
            "session directory `{}` contains slug `{}`",
            path.slug.as_str(),
            session.slug.as_str()
        )))
    }
}

fn session_io_error(action: &str, path: &Path, source: io::Error) -> BleatError {
    BleatError::Runtime(format!("failed to {action} `{}`: {source}", path.display()))
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

    #[test]
    fn create_session_rejects_an_existing_slug() {
        let sandbox = tempdir().expect("sandbox should be created");
        let slug = || Slug::parse("feature").expect("slug should be valid");
        let role = || Role::parse("claude").expect("role should be valid");
        create_session(
            sandbox.path(),
            slug(),
            role(),
            timestamp("2026-07-15T10:00:00+09:00"),
        )
        .expect("first creation should succeed");

        let error = create_session(
            sandbox.path(),
            slug(),
            role(),
            timestamp("2026-07-15T11:00:00+09:00"),
        )
        .expect_err("duplicate slug should fail");

        assert!(matches!(error, BleatError::Usage(_)));
    }

    #[test]
    fn create_session_writes_a_loadable_persistent_layout() {
        let sandbox = tempdir().expect("sandbox should be created");
        let path = create_test_session(sandbox.path(), "feature", "2026-07-15T10:00:00+09:00");

        let session = load_session(&path.session_json()).expect("session should load");

        assert_eq!(session.slug.as_str(), "feature");
        assert!(session.roles.keys().any(|role| role.as_str() == "claude"));
        assert!(path.messages_dir().is_dir());
    }

    #[test]
    fn resolve_session_prefers_the_flag_over_the_environment() {
        let sandbox = tempdir().expect("sandbox should be created");
        create_test_session(sandbox.path(), "alpha", "2026-07-15T09:00:00+09:00");
        create_test_session(sandbox.path(), "beta", "2026-07-15T10:00:00+09:00");

        let resolution = resolve_session(sandbox.path(), Some("beta"), Some("alpha"))
            .expect("flag session should resolve");

        assert_eq!(resolution.path.slug().as_str(), "beta");
    }

    #[test]
    fn resolve_session_uses_the_environment_without_a_flag() {
        let sandbox = tempdir().expect("sandbox should be created");
        create_test_session(sandbox.path(), "alpha", "2026-07-15T09:00:00+09:00");
        create_test_session(sandbox.path(), "beta", "2026-07-15T10:00:00+09:00");

        let resolution = resolve_session(sandbox.path(), None, Some("alpha"))
            .expect("environment session should resolve");

        assert_eq!(resolution.path.slug().as_str(), "alpha");
    }

    #[test]
    fn resolve_session_uses_the_latest_created_session_by_default() {
        let sandbox = tempdir().expect("sandbox should be created");
        create_test_session(sandbox.path(), "alpha", "2026-07-15T09:00:00+09:00");
        create_test_session(sandbox.path(), "beta", "2026-07-15T10:00:00+09:00");

        let resolution =
            resolve_session(sandbox.path(), None, None).expect("latest session should resolve");

        assert_eq!(resolution.path.slug().as_str(), "beta");
    }

    #[test]
    fn resolve_session_breaks_created_ties_by_slug() {
        let sandbox = tempdir().expect("sandbox should be created");
        create_test_session(sandbox.path(), "beta", "2026-07-15T10:00:00+09:00");
        create_test_session(sandbox.path(), "alpha", "2026-07-15T10:00:00+09:00");

        let resolution =
            resolve_session(sandbox.path(), None, None).expect("tied session should resolve");

        assert_eq!(resolution.path.slug().as_str(), "alpha");
    }

    #[test]
    fn resolve_session_warns_when_multiple_sessions_exist() {
        let sandbox = tempdir().expect("sandbox should be created");
        create_test_session(sandbox.path(), "alpha", "2026-07-15T09:00:00+09:00");
        create_test_session(sandbox.path(), "beta", "2026-07-15T10:00:00+09:00");

        let resolution =
            resolve_session(sandbox.path(), None, None).expect("latest session should resolve");

        assert!(resolution.warning.is_some());
    }

    #[test]
    fn resolve_session_reports_corrupt_session_data() {
        let sandbox = tempdir().expect("sandbox should be created");
        let broken = sandbox.path().join(".bleat/broken");
        fs::create_dir_all(&broken).expect("broken session dir should be created");
        fs::write(broken.join("session.json"), "not-json")
            .expect("broken session should be written");

        let error = resolve_session(sandbox.path(), None, None)
            .expect_err("corrupt session should not be ignored");

        assert!(matches!(error, BleatError::Runtime(_)));
    }

    #[test]
    fn resolve_session_treats_an_invalid_directory_slug_as_corrupt_data() {
        let sandbox = tempdir().expect("sandbox should be created");
        fs::create_dir_all(sandbox.path().join(".bleat/INVALID"))
            .expect("invalid session dir should be created");

        let error = resolve_session(sandbox.path(), None, None)
            .expect_err("invalid stored slug should not be a usage error");

        assert!(matches!(error, BleatError::Runtime(_)));
        assert!(error.to_string().contains("invalid session directory"));
    }

    fn timestamp(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).expect("timestamp fixture should be valid")
    }

    fn create_test_session(root: &Path, slug: &str, created: &str) -> SessionPath {
        create_session(
            root,
            Slug::parse(slug).expect("slug should be valid"),
            Role::parse("claude").expect("role should be valid"),
            timestamp(created),
        )
        .expect("session should be created")
    }
}
