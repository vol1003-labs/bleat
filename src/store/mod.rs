use chrono::{DateTime, FixedOffset};

use crate::error::BleatError;
use crate::identity::{MessageType, Role};
use crate::message::Message;

pub mod file;

pub struct Draft {
    pub from: Role,
    pub to: Role,
    pub kind: MessageType,
    pub reply_to: Option<u64>,
    pub timestamp: DateTime<FixedOffset>,
    pub body: String,
}

pub trait Store {
    fn publish(&self, draft: Draft) -> Result<Message, BleatError>;
    fn all(&self) -> Result<Vec<Message>, BleatError>;
    fn cursor(&self, role: &Role) -> Result<u64, BleatError>;
    fn read_unread(&self, role: &Role, peek: bool) -> Result<Vec<Message>, BleatError>;
    fn unread_count(&self, role: &Role) -> Result<usize, BleatError>;
}
