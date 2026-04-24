//! Store-local semantic input commands.

mod command;
mod resolve;

pub(crate) use command::Command;
pub(crate) use resolve::resolve_command;
