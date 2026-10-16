//! EM-safe declarative mod manifests and fixture runner for Bicameral.
//!
//! Mods are declarative configuration units that EMs can author to express
//! governance routing, risk signals, and integration behavior without writing
//! code. They operate within strict authority boundaries: mods may emit
//! candidates, evidence, hints, and advisory warnings, but may never directly
//! create canonical decisions or approve their own authority expansion.
//!
//! Derived from ZeroClaw's plugin/SOP concepts, narrowed to EM-safe governance.
//! See NOTICE and runtime/UPSTREAM-ZEROCLAW.md for attribution.

pub mod manifest;
pub mod runner;
pub mod validate;

pub use manifest::{ModAction, ModManifest, ModTrigger};
pub use runner::FixtureRunner;
pub use validate::validate_manifest;
