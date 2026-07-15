use std::ffi::OsString;
use std::path::Path;

use crate::error::BleatError;
use crate::identity::{Role, Slug};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeHandle {
    pub terminal_id: String,
    pub pane_id: String,
    pub agent_name: Option<String>,
}

pub trait Runtime {
    fn current_handle(&self) -> Result<RuntimeHandle, BleatError>;

    fn spawn(
        &self,
        slug: &Slug,
        role: &Role,
        cwd: &Path,
        argv: &[OsString],
    ) -> Result<RuntimeHandle, BleatError>;

    fn alive(&self, handle: &RuntimeHandle) -> Result<bool, BleatError>;

    fn nudge(&self, handle: &RuntimeHandle, slug: &Slug, role: &Role) -> Result<(), BleatError>;
}
