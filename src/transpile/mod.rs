//! Transpilation module - generate formal specifications from Intent.
//!
//! This module contains transpilers that convert Intent behaviors to
//! various formal specification languages.

pub mod tla;

// Re-export key types from tla
pub use tla::{
    generate_for_apalache, generate_with_tlc_config, StateMachineTla, TlaConfig, TlcConfig,
};
