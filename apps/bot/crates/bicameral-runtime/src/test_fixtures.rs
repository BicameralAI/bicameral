//! Committed fixture helpers for V1 code-grounding tests.
//!
//! Copies the committed `tests/fixtures/code-grounding/` workspace into a
//! temporary git repository so that runtime/gateway tests can exercise every
//! `GraphEvidenceState` variant without network access or external repos.
//!
//! All outputs produced through these helpers are **evidence states** and
//! **graph-readiness observations**, not compliance verdicts, enforcement
//! decisions, or feature-readiness claims.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Root of the committed fixture workspace relative to the workspace Cargo root.
const FIXTURE_DIR: &str = "tests/fixtures/code-grounding";

/// Locate the fixture directory by walking up from the runtime crate's
/// manifest dir until we find the workspace root containing the fixture.
fn locate_fixture_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR for bicameral-runtime is crates/bicameral-runtime.
    // The workspace root is two levels up.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("cannot locate workspace root from CARGO_MANIFEST_DIR");
    let fixture = workspace_root.join(FIXTURE_DIR);
    assert!(
        fixture.exists(),
        "fixture directory not found at {}: run from workspace root",
        fixture.display()
    );
    fixture
}

/// Copy a directory tree recursively (files only, no symlinks).
fn copy_dir_all(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            copy_dir_all(&entry.path(), &dest);
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

fn git(args: &[&str], dir: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} in {} failed: {}",
        args,
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A temporary git repository initialised from the committed fixture.
///
/// Tests can call mutation helpers to produce specific evidence states
/// (stale, moved, missing, etc.) after construction.
pub struct FixtureWorktree {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Default for FixtureWorktree {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureWorktree {
    /// Copy the committed fixture into a fresh temp git repo and commit.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();

        // Initialise a fresh git repo.
        git(&["init"], &p);
        git(&["config", "user.email", "fixture@test.com"], &p);
        git(&["config", "user.name", "Fixture"], &p);

        // Copy fixture content.
        let fixture = locate_fixture_dir();
        copy_dir_all(&fixture, &p);

        // Commit the initial state.
        git(&["add", "."], &p);
        git(&["commit", "-m", "fixture: initial committed state"], &p);

        Self { _dir: dir, path: p }
    }

    /// Filesystem path to the temporary worktree root.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // ── Mutation helpers ──────────────────────────────────────────────

    /// Dirty a file without committing → produces `UnknownStale`.
    pub fn dirty_file(&self, relative: &str, content: &str) {
        let target = self.path.join(relative);
        assert!(target.exists(), "cannot dirty non-existent {relative}");
        std::fs::write(&target, content).unwrap();
    }

    /// Remove a file and commit → binding the old path produces `NotFound`.
    pub fn remove_and_commit(&self, relative: &str, message: &str) {
        let target = self.path.join(relative);
        std::fs::remove_file(&target).unwrap();
        git(&["add", "-A"], &self.path);
        git(&["commit", "-m", message], &self.path);
    }

    /// Rename a file and commit → old path `NotFound`, new path `Verified`.
    pub fn rename_and_commit(&self, from: &str, to: &str, message: &str) {
        let src = self.path.join(from);
        let dst_path = self.path.join(to);
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::rename(&src, &dst_path).unwrap();
        git(&["add", "-A"], &self.path);
        git(&["commit", "-m", message], &self.path);
    }

    /// Write a new file and commit.
    pub fn add_file_and_commit(&self, relative: &str, content: &str, message: &str) {
        let target = self.path.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&target, content).unwrap();
        git(&["add", "-A"], &self.path);
        git(&["commit", "-m", message], &self.path);
    }
}
