//! Audit receipt recording for Bicameral governance actions.
//!
//! Every governance action (ingest, review command, policy evaluation, event
//! materialization) produces an audit receipt. Receipts form a tamper-evident
//! chain that can be replayed to understand how decisions reached their
//! current state.
//!
//! Derived from ZeroClaw's tool-receipt and SOP audit concepts.
//! See NOTICE and runtime/UPSTREAM-ZEROCLAW.md for attribution.

pub mod receipt;
pub mod store;

pub use receipt::{AuditAction, AuditReceipt};
pub use store::AuditStore;
