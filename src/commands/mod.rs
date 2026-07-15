pub mod init;
pub mod join;
pub mod log;
pub mod read;
pub mod send;

use crate::message::{Message, encode};

fn format_messages(messages: &[Message]) -> String {
    let mut output = String::new();
    for (index, message) in messages.iter().enumerate() {
        if index > 0 {
            output.push_str("\n\n");
        }
        output.push_str(&encode(message));
    }
    output
}
