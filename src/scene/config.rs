//! Runtime configuration parsing and validation for the scene triple-buffer pipeline.

use std::fmt;
use std::fs;
use std::path::Path;

use log::info;
use serde::Deserialize;

const MIN_ARENA_INITIAL_CHUNK_BYTES: usize = 4096;
const MAX_ARENA_INITIAL_CHUNK_BYTES: usize = 1 << 22;
const MAX_REBUILD_BUDGET_MS: u32 = 4;
const MAX_COMPOSE_BUDGET_MS: u32 = 8;
const MAX_END_TO_END_LATENCY_MS: u32 = 33;
const MAX_BLOCKS_PER_SCENE: usize = 512;

#[derive(Clone, Debug, Deserialize)]
struct RootConfig {
    #[serde(default)]
    scene: SceneConfig,
}

/// Runtime configuration for the triple-buffer scene pipeline.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub(crate) struct SceneConfig {
    pub(crate) arena_initial_chunk_bytes: usize,
    pub(crate) rebuild_budget_ms: u32,
    pub(crate) compose_budget_ms: u32,
    pub(crate) end_to_end_latency_ms: u32,
    pub(crate) max_blocks_per_scene: usize,
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self {
            arena_initial_chunk_bytes: 65_536,
            rebuild_budget_ms: MAX_REBUILD_BUDGET_MS,
            compose_budget_ms: MAX_COMPOSE_BUDGET_MS,
            end_to_end_latency_ms: MAX_END_TO_END_LATENCY_MS,
            max_blocks_per_scene: MAX_BLOCKS_PER_SCENE,
        }
    }
}

impl SceneConfig {
    /// Loads `[scene]` from `config.toml`, or falls back to documented defaults when missing.
    pub(crate) fn load_or_default(path: impl AsRef<Path>) -> Result<Self, SceneConfigError> {
        let path = path.as_ref();
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    "config.load path={} mode=defaults reason=not_found",
                    path.display()
                );
                let config = Self::default();
                config.validate()?;
                return Ok(config);
            }
            Err(error) => return Err(SceneConfigError::Io(error)),
        };

        let root: RootConfig = toml::from_str(&contents).map_err(SceneConfigError::Parse)?;
        root.scene.validate()?;
        Ok(root.scene)
    }

    fn validate(&self) -> Result<(), SceneConfigError> {
        validate_range(
            "arena_initial_chunk_bytes",
            self.arena_initial_chunk_bytes,
            MIN_ARENA_INITIAL_CHUNK_BYTES,
            MAX_ARENA_INITIAL_CHUNK_BYTES,
        )?;
        validate_max(
            "rebuild_budget_ms",
            self.rebuild_budget_ms,
            MAX_REBUILD_BUDGET_MS,
        )?;
        validate_max(
            "compose_budget_ms",
            self.compose_budget_ms,
            MAX_COMPOSE_BUDGET_MS,
        )?;
        validate_max(
            "end_to_end_latency_ms",
            self.end_to_end_latency_ms,
            MAX_END_TO_END_LATENCY_MS,
        )?;
        validate_max(
            "max_blocks_per_scene",
            self.max_blocks_per_scene,
            MAX_BLOCKS_PER_SCENE,
        )?;
        Ok(())
    }
}

fn validate_range<T>(field: &'static str, value: T, min: T, max: T) -> Result<(), SceneConfigError>
where
    T: Copy + fmt::Display + Ord,
{
    if value < min {
        return Err(SceneConfigError::InvalidValue {
            message: format!("{field} must be >= {min}"),
        });
    }
    if value > max {
        return Err(SceneConfigError::InvalidValue {
            message: format!("{field} must be <= {max}"),
        });
    }
    Ok(())
}

fn validate_max<T>(field: &'static str, value: T, max: T) -> Result<(), SceneConfigError>
where
    T: Copy + fmt::Display + Ord,
{
    if value > max {
        return Err(SceneConfigError::InvalidValue {
            message: format!("{field} must be <= {max}"),
        });
    }
    Ok(())
}

/// Errors raised while loading or validating scene runtime configuration.
#[derive(Debug)]
pub(crate) enum SceneConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    InvalidValue { message: String },
}

impl fmt::Display for SceneConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read config.toml: {error}"),
            Self::Parse(error) => write!(f, "failed to parse config.toml: {error}"),
            Self::InvalidValue { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for SceneConfigError {}
