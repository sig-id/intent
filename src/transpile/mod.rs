//! Transpilation module - generate formal specifications from Intent.
//!
//! This module contains transpilers that convert Intent behaviors to
//! various formal specification languages.

pub mod executable;
pub mod tla;

pub use executable::generate_executable_v2;

// Re-export key types from tla
pub use tla::{
    generate_for_apalache, generate_with_tlc_config, Obligation, ObligationManifest,
    RefinementArtifact, StateMachineTla, TlaConfig, TlcConfig,
};
