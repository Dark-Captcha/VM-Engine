//! Crate-level error types.

// ============================================================================
// Imports
// ============================================================================

use thiserror::Error;

// ============================================================================
// Error
// ============================================================================

/// Errors produced by IR construction, validation, and execution.
#[derive(Error, Debug)]
pub enum Error {
    #[error("validation: {0}")]
    Validation(String),

    #[error("build: {0}")]
    Build(String),

    #[error("execution: {0}")]
    Exec(String),
}

impl Error {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn build(msg: impl Into<String>) -> Self {
        Self::Build(msg.into())
    }

    pub fn exec(msg: impl Into<String>) -> Self {
        Self::Exec(msg.into())
    }
}

/// Crate-level result alias.
pub type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_category() {
        let e = Error::validation("missing terminator in block B0");
        assert!(e.to_string().starts_with("validation:"));
    }
}
