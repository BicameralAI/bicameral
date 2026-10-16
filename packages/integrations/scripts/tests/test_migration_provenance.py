# SPDX-License-Identifier: Apache-2.0
"""Migration-aware provenance: synthetic source and monorepo repositories exercise
every acceptance and rejection path without depending on the real history."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

_REPO = Path(__file__).resolve().parents[2]
for extra in (str(_REPO), str(_REPO / "scripts")):
    if extra not in sys.path:
        sys.path.insert(0, extra)

_spec = importlib.util.spec_from_file_location("migration_provenance", _REPO / "scripts" / "migration_provenance.py")
migration = importlib.util.module_from_spec(_spec)
sys.modules.setdefault("migration_provenance", migration)
_spec.loader.exec_module(migration)

SOURCE = "ExampleOrg/example-component"
DEST = "packages/example"
COMPONENT = "lib/core.py"


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
        capture_output=True, text=True, check=True,
    ).stdout.strip()


def _commit(repo: Path, files: dict[str, str], message: str) -> str:
    for rel, content in files.items():
        target = repo / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", message)
    return _git(repo, "rev-parse", "HEAD")


class Fixture:
    def __init__(self, tmp: Path) -> None:
        self.source = tmp / "source"
        self.source.mkdir()
        _git(self.source, "init", "-q", "-b", "main")
        self.reviewed = _commit(self.source, {COMPONENT: "VALUE = 1\n"}, "reviewed")
        self.snapshot = _commit(self.source, {"README.md": "readme\n"}, "later, component unchanged")
        _git(self.source, "checkout", "-q", "--orphan", "unrelated")
        self.unrelated = _commit(self.source, {COMPONENT: "VALUE = 1\n"}, "not an ancestor")
        _git(self.source, "checkout", "-q", "main")

        self.root = tmp / "mono"
        self.component = self.root / DEST
        self.component.mkdir(parents=True)
        _git(self.root, "init", "-q", "-b", "main")
        (self.component / "lib").mkdir()
        (self.component / COMPONENT).write_text("VALUE = 1\n", encoding="utf-8")
        (self.component / "README.md").write_text("readme\n", encoding="utf-8")

        digest = migration.blob_digest(self.source, self.reviewed, COMPONENT)
        self.bundle = {
            "bundle_id": "sha256:" + "b" * 64,
            "integrations": {"repository": SOURCE, "commit": self.reviewed},
            "implementation_provenance": {
                "repository": SOURCE,
                "reviewed_commit": self.reviewed,
                "components": [{"path": COMPONENT, "blob_sha256": digest}],
            },
            "stages": [],
        }
        self.write_sources(self.snapshot)
        self.manifest = migration.build_manifest(self.source, SOURCE, self.snapshot, DEST)
        self.verification = migration.build_verification(
            self.bundle, "bundle.json", self.source, SOURCE, self.snapshot, self.manifest
        )
        self.save()
        self.head = self._commit_all()

    def _commit_all(self) -> str:
        _git(self.root, "add", "-A")
        _git(self.root, "commit", "-q", "-m", "import snapshot")
        return _git(self.root, "rev-parse", "HEAD")

    def write_sources(self, sha: str) -> None:
        sources = {"components": [{"source": SOURCE, "sha": sha, "destination": DEST}]}
        path = self.root / "migration" / "sources.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(sources), encoding="utf-8")

    def save(self) -> None:
        for path, data in (
            (migration.manifest_path(self.root, SOURCE), self.manifest),
            (migration.verification_path(self.root, SOURCE), self.verification),
        ):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(data), encoding="utf-8")

    def reseal_verification(self) -> None:
        self.verification["verification_digest"] = migration.canonical_digest(
            self.verification, "verification_digest"
        )

    def errors(self, *, source_repo: Path | None = None) -> list[str]:
        return migration.migration_provenance_errors(
            self.bundle, self.component, head=self.head, source_repo=source_repo
        )


@pytest.fixture
def fx(tmp_path: Path) -> Fixture:
    return Fixture(tmp_path)


def _has(errors: list[str], fragment: str) -> bool:
    return any(fragment in error for error in errors)


# --- acceptance ------------------------------------------------------------


def test_valid_snapshot_passes_offline(fx: Fixture) -> None:
    assert fx.errors() == []


def test_valid_snapshot_passes_against_source_history(fx: Fixture) -> None:
    assert fx.errors(source_repo=fx.source) == []


def test_mode_selection_uses_migration_evidence_without_local_history(fx: Fixture) -> None:
    assert not migration.commit_exists(fx.component, fx.reviewed)
    assert migration.provenance_errors(fx.bundle, fx.component, head=fx.head) == []


def test_original_approval_is_referenced_not_restated(fx: Fixture) -> None:
    assert fx.verification["is_original_approval"] is False
    assert fx.verification["original_approval"]["reviewed_commit"] == fx.reviewed
    assert fx.verification["source"]["sha"] == fx.snapshot
    assert "not a review or approval" in fx.verification["scope"]


def test_manifest_is_deterministic(fx: Fixture) -> None:
    again = migration.build_manifest(fx.source, SOURCE, fx.snapshot, DEST)
    assert again == fx.manifest
    assert [f["path"] for f in fx.manifest["files"]] == ["README.md", COMPONENT]


def test_reviewed_blob_digest_only_for_the_verified_commit(fx: Fixture) -> None:
    digest = fx.bundle["implementation_provenance"]["components"][0]["blob_sha256"]
    assert migration.reviewed_blob_digest(fx.component, fx.reviewed, COMPONENT) == digest
    assert migration.reviewed_blob_digest(fx.component, fx.snapshot, COMPONENT) is None
    assert migration.reviewed_blob_digest(fx.component, fx.reviewed, "lib/other.py") is None


# --- original approval cannot be altered or impersonated -------------------


def test_altered_reviewed_commit_is_rejected(fx: Fixture) -> None:
    fx.bundle["implementation_provenance"]["reviewed_commit"] = fx.snapshot
    fx.bundle["integrations"]["commit"] = fx.snapshot
    assert _has(fx.errors(), "not the commit the migration verification covers")


def test_verification_claiming_original_approval_is_rejected(fx: Fixture) -> None:
    fx.verification["is_original_approval"] = True
    fx.reseal_verification()
    fx.save()
    assert _has(fx.errors(), "must not claim to be an original approval")


def test_altered_verification_is_rejected(fx: Fixture) -> None:
    fx.verification["components"][0]["blob_sha256_at_source_sha"] = "sha256:" + "0" * 64
    fx.save()
    assert _has(fx.errors(), "verification digest mismatch")


def test_verification_for_another_bundle_is_rejected(fx: Fixture) -> None:
    fx.bundle["bundle_id"] = "sha256:" + "c" * 64
    assert _has(fx.errors(), "covers a different bundle")


def test_integrations_commit_must_still_equal_reviewed_commit(fx: Fixture) -> None:
    fx.bundle["integrations"]["commit"] = fx.snapshot
    assert _has(fx.errors(), "integrations.commit must equal")


# --- unknown source SHAs and altered or missing evidence --------------------


def test_unknown_source_sha_is_rejected(fx: Fixture) -> None:
    fx.write_sources("f" * 40)
    errors = fx.errors()
    assert _has(errors, "unknown source SHA")


def test_malformed_source_sha_is_rejected(fx: Fixture) -> None:
    fx.write_sources("not-a-sha")
    assert _has(fx.errors(), "unknown source SHA")


def test_altered_manifest_is_rejected(fx: Fixture) -> None:
    fx.manifest["files"][0]["sha256"] = "sha256:" + "1" * 64
    fx.save()
    assert _has(fx.errors(), "manifest digest mismatch")


def test_missing_verification_is_rejected(fx: Fixture) -> None:
    migration.verification_path(fx.root, SOURCE).unlink()
    assert _has(fx.errors(), "missing migration evidence")


def test_missing_manifest_is_rejected(fx: Fixture) -> None:
    migration.manifest_path(fx.root, SOURCE).unlink()
    assert _has(fx.errors(), "missing migration evidence")


def test_component_without_migration_record_is_rejected(fx: Fixture) -> None:
    (fx.root / "migration" / "sources.json").write_text(json.dumps({"components": []}), encoding="utf-8")
    assert _has(fx.errors(), "no migration record")


# --- content mapping --------------------------------------------------------


def test_component_changed_after_import_is_rejected(fx: Fixture) -> None:
    (fx.component / COMPONENT).write_text("VALUE = 2\n", encoding="utf-8")
    errors = fx.errors()
    assert _has(errors, "changed after import")
    assert _has(errors, "changed after reviewed_commit")


def test_component_missing_from_monorepo_is_rejected(fx: Fixture) -> None:
    (fx.component / COMPONENT).unlink()
    assert _has(fx.errors(), "changed after import")


def test_component_absent_from_verification_is_rejected(fx: Fixture) -> None:
    fx.bundle["implementation_provenance"]["components"].append(
        {"path": "lib/new.py", "blob_sha256": "sha256:" + "2" * 64}
    )
    errors = fx.errors()
    assert _has(errors, "absent at reviewed_commit per migration verification")
    assert _has(errors, "absent from the imported snapshot manifest")


# --- historical ancestry against the original history ----------------------


def test_forged_ancestry_is_caught_when_source_history_is_available(fx: Fixture) -> None:
    """Offline, a sealed verification is trusted as recorded; the original
    history is the authority whenever it is available."""
    for record in (fx.bundle["implementation_provenance"], fx.verification["original_approval"]):
        record["reviewed_commit"] = fx.unrelated
    fx.bundle["integrations"]["commit"] = fx.unrelated
    fx.reseal_verification()
    fx.save()
    assert fx.errors() == []
    assert _has(fx.errors(source_repo=fx.source), "is not an ancestor of the imported source SHA")


def test_source_sha_unknown_to_source_history_is_rejected(fx: Fixture, tmp_path: Path) -> None:
    other = tmp_path / "other-source"
    other.mkdir()
    _git(other, "init", "-q")
    _commit(other, {"x": "y\n"}, "unrelated repository")
    assert _has(fx.errors(source_repo=other), "unknown source SHA")


def test_manifest_not_matching_source_snapshot_is_rejected(fx: Fixture) -> None:
    fx.manifest["files"][0]["sha256"] = "sha256:" + "1" * 64
    fx.manifest["manifest_digest"] = migration.canonical_digest(fx.manifest, "manifest_digest")
    fx.verification["manifest_digest"] = fx.manifest["manifest_digest"]
    fx.reseal_verification()
    fx.save()
    assert _has(fx.errors(source_repo=fx.source), "manifest does not match the source snapshot")


def test_records_are_never_built_for_a_non_ancestor(fx: Fixture) -> None:
    fx.bundle["implementation_provenance"]["reviewed_commit"] = fx.unrelated
    with pytest.raises(SystemExit, match="not an ancestor"):
        migration.build_verification(fx.bundle, "bundle.json", fx.source, SOURCE, fx.snapshot, fx.manifest)


def test_records_are_never_built_for_an_unknown_source_sha(fx: Fixture) -> None:
    with pytest.raises(SystemExit, match="unknown source SHA"):
        migration.build_manifest(fx.source, SOURCE, "e" * 40, DEST)
