use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, StackForgeError>;

#[derive(Debug, Error)]
pub enum StackForgeError {
    #[error("recipe `{recipe}` is invalid: {reason}")]
    InvalidRecipe { recipe: String, reason: String },

    #[error("recipe `{0}` was not found in the configured registry")]
    RecipeNotFound(String),

    #[error("recipe dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("recipe conflict: `{left}` cannot be combined with `{right}`")]
    Conflict { left: String, right: String },

    #[error("recipe `{recipe}` is not compatible with `{selected}`")]
    Incompatible { recipe: String, selected: String },

    #[error("required capability `{capability}` for recipe `{recipe}` is unavailable")]
    MissingCapability { recipe: String, capability: String },

    #[error("unsafe path `{path}` in recipe `{recipe}`")]
    UnsafePath { recipe: String, path: String },

    #[error("template rendering failed for recipe `{recipe}`, file `{file}`: {reason}")]
    Render {
        recipe: String,
        file: String,
        reason: String,
    },

    #[error("file conflict at `{path}`: {reason}")]
    FileConflict { path: PathBuf, reason: String },

    #[error("transaction failed while applying `{path}`: {reason}")]
    Transaction { path: PathBuf, reason: String },

    #[error("project is not managed by StackForge: `{0}` is missing")]
    MissingState(PathBuf),

    #[error("state file is invalid: {0}")]
    InvalidState(String),

    #[error("input validation failed: {0}")]
    InvalidInput(String),

    #[error("I/O error at `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("serialization error: {0}")]
    Serialization(String),
}

impl StackForgeError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
