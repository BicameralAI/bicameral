/// Baseline file for rename/move scenario.
///
/// Tests copy the fixture, commit it, then rename this file to
/// `src/renamed.rs` and re-commit. Binding the old path at the new
/// snapshot produces `NotFound`; binding the new path produces `Verified`.
pub fn stable_function() -> &'static str {
    "original location"
}
