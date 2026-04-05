//! High-level renderer state and lifecycle orchestration for the M0 prototype.

mod bind_group;
mod buffer;
mod frame;
mod rebuild;
mod state;

pub(crate) use state::Renderer;
