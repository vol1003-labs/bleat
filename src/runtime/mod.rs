use std::ffi::OsString;
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::error::BleatError;
use crate::identity::{Role, Slug};

pub mod herdr;

pub trait RuntimeHandle: Serialize + DeserializeOwned {
    fn into_value(self) -> Result<Value, BleatError>
    where
        Self: Sized,
    {
        serde_json::to_value(self)
            .map_err(|source| BleatError::Runtime(format!("encode runtime handle: {source}")))
    }

    fn from_value(value: Value) -> Result<Self, BleatError>
    where
        Self: Sized,
    {
        serde_json::from_value(value)
            .map_err(|source| BleatError::Runtime(format!("decode runtime handle: {source}")))
    }
}

pub trait Runtime {
    type Handle: RuntimeHandle;

    fn current_handle(&self) -> Result<Self::Handle, BleatError>;

    fn spawn(
        &self,
        slug: &Slug,
        role: &Role,
        cwd: &Path,
        argv: &[OsString],
    ) -> Result<Self::Handle, BleatError>;

    fn alive(&self, handle: &Self::Handle) -> Result<bool, BleatError>;

    fn nudge(&self, handle: &Self::Handle, slug: &Slug, role: &Role) -> Result<(), BleatError>;
}
