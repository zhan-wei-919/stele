//! Scene-buffer runtime types shared by the store, view protocol, and renderer.

mod block;
mod buffer;
mod config;
pub(crate) mod instance;
mod pipeline;
mod pool;
mod protocol;

pub(crate) const SCENE_BUFFER_SLOTS: usize = 3;
pub(crate) const ATLAS_SLACK: usize = 1;
pub(crate) const VIEW_UPDATE_CHANNEL_CAPACITY: usize = SCENE_BUFFER_SLOTS + ATLAS_SLACK;

pub(crate) use block::{BlockDataArena, BlockId};
pub(crate) use buffer::{SceneBuffer, SceneBufferInner, SceneFrameMetadata};
pub(crate) use config::SceneConfig;
pub(crate) use pipeline::ScenePipeline;
pub(crate) use pool::SceneBufferPool;
pub(crate) use protocol::SceneProtocolState;
