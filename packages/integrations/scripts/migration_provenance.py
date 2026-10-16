#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Migration-aware implementation provenance for information-cycle bundles.

This module exists only for the consolidation of ``BicameralAI/bicameral-integrations``
into the Bicameral monorepo (BicameralAI/bicameral-factory#969). The monorepo carries
the Integrations source as a snapshot without Git history, so the bundle's
``implementation_provenance.reviewed_commit`` cannot be resolved locally.

It does not change the bundle, the validator, or the generator: those files are
reviewed components whose blob digests the bundle records. It composes the
validator and replaces only its provenance step.

Two modes, selected by where the component lives:

* **Original mode.** In the standalone source repository, the validator's own
  ``provenance_errors`` runs unchanged against that repository's history.
* **Migration mode.** Inside the monorepo, provenance is always checked against
  migration evidence at the monorepo root, even if ``reviewed_commit`` happens to
  exist in the monorepo history:

  - ``migration/sources.json``: the imported snapshot's source repository and SHA.
  - ``migration/manifests/<repo>.json``: a deterministic manifest of every file at
    that SHA with its content hash.
  - ``migration/verifications/<repo>.information-cycle-provenance.json``: a
    migration verification recording that ``reviewed_commit`` is an ancestor of the
    imported SHA and that every component's bytes are identical at both commits.

  The original approval (``reviewed_commit``, repository and bundle) stays exactly
  as recorded. The migration verification references it and is marked
  ``is_original_approval: false``. It can never stand in for a review. When the
  original repository is available (``--source-repo`` or the
  ``BICAMERAL_INTEGRATIONS_SOURCE_REPO`` environment variable), ancestry, component
  blobs and the manifest are re-verified against its history.

Trust boundary: offline, the records are self-consistent hashes, not signatures.
Someone who edits a record and recomputes every digest can only be caught by code
review (``migration/`` and ``packages/integrations/`` are code-owned) or by running
against the original history. The original history is authoritative whenever it
is available.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess  # nosec B404 - fixed-argv git plumbing only
import sys
from collections import Counter
from hashlib import sha256
from pathlib import Path
from typing import Any

_REPO = Path(__file__).resolve().parents[1]
for _extra in (str(_REPO), str(_REPO / "scripts")):
    if _extra not in sys.path:
        sys.path.insert(0, _extra)

import validate_information_cycle_bundle as validator  # noqa: E402

SOURCE_ENV = "BICAMERAL_INTEGRATIONS_SOURCE_REPO"
MANIFEST_KIND = "bicameral.migration.snapshot-manifest"
VERIFICATION_KIND = "bicameral.migration.provenance-verification"
VERIFICATION_SCOPE = (
    "Migration compatibility only: confirms that the originally reviewed commit is an "
    "ancestor of the imported source snapshot and that every reviewed component is "
    "byte-identical at both. It is not a review or approval of the implementation."
)
_SHA_RE = re.compile(r"^[0-9a-f]{40}$")


# ---------------------------------------------------------------------------
# Primitives


def _git(repo: Path, *args: str) -> tuple[int, bytes]:
    completed = subprocess.run(  # nosec B603 B607
        ["git", "-C", str(repo), *args], capture_output=True, check=False
    )
    return completed.returncode, completed.stdout


def commit_exists(repo: Path, sha: str) -> bool:
    if not _SHA_RE.match(sha or ""):
        return False
    code, _ = _git(repo, "cat-file", "-e", f"{sha}^{{commit}}")
    return code == 0


def blob_digest(repo: Path, commit: str, path: str) -> str | None:
    """Normalized digest (the bundle's convention) of ``path`` at ``commit``.

    ``./`` makes the path relative to ``repo`` rather than to the Git root, so the
    same call works in the source repository and inside the monorepo."""
    code, out = _git(repo, "show", f"{commit}:./{path}")
    if code != 0:
        return None
    return "sha256:" + sha256(out.replace(b"\r\n", b"\n")).hexdigest()


def raw_digest(data: bytes) -> str:
    return "sha256:" + sha256(data).hexdigest()


def canonical_digest(record: dict[str, Any], exclude: str) -> str:
    unsigned = {k: v for k, v in record.items() if k != exclude}
    canon = json.dumps(unsigned, sort_keys=True, separators=(",", ":"))
    return "sha256:" + sha256(canon.encode("utf-8")).hexdigest()


def _source_name(source_repository: str) -> str:
    return source_repository.rsplit("/", 1)[-1]


# ---------------------------------------------------------------------------
# Migration context


def locate(repo: Path) -> tuple[Path, str] | None:
    """Return (monorepo root, component destination) when ``repo`` is a
    subdirectory of a Git work tree; ``None`` for a standalone repository."""
    code, out = _git(repo, "rev-parse", "--show-toplevel")
    if code != 0:
        return None
    root = Path(out.decode().strip()).resolve()
    try:
        destination = repo.resolve().relative_to(root).as_posix()
    except ValueError:
        return None
    if destination in ("", "."):
        return None
    return root, destination


def manifest_path(root: Path, source_repository: str) -> Path:
    return root / "migration" / "manifests" / f"{_source_name(source_repository)}.json"


def verification_path(root: Path, source_repository: str) -> Path:
    name = _source_name(source_repository)
    return root / "migration" / "verifications" / f"{name}.information-cycle-provenance.json"


def _load_json(path: Path) -> dict[str, Any] | None:
    if not path.is_file() or path.is_symlink():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except ValueError:
        return None
    return data if isinstance(data, dict) else None


def load_migration(repo: Path) -> tuple[dict[str, Any] | None, list[str]]:
    """Load the migration evidence for the component at ``repo``."""
    located = locate(repo)
    if located is None:
        return None, ["no migration record: repository is not a monorepo component"]
    root, destination = located
    sources = _load_json(root / "migration" / "sources.json")
    if sources is None:
        return None, ["missing migration evidence: migration/sources.json"]
    entries = [c for c in sources.get("components", []) if c.get("destination") == destination]
    if len(entries) != 1:
        return None, [f"no migration record for {destination} in migration/sources.json"]
    entry = entries[0]
    source_repository = entry.get("source", "")
    errors: list[str] = []
    manifest = _load_json(manifest_path(root, source_repository))
    if manifest is None:
        errors.append(f"missing migration evidence: {manifest_path(root, source_repository).relative_to(root)}")
    verification = _load_json(verification_path(root, source_repository))
    if verification is None:
        errors.append(
            f"missing migration evidence: {verification_path(root, source_repository).relative_to(root)}"
        )
    context = {
        "root": root,
        "destination": destination,
        "entry": entry,
        "manifest": manifest,
        "verification": verification,
    }
    return context, errors


# ---------------------------------------------------------------------------
# Record construction (requires the original source repository)


def build_manifest(source_repo: Path, source_repository: str, source_sha: str, destination: str) -> dict[str, Any]:
    """Deterministic manifest of every file at ``source_sha`` with raw content hashes."""
    if not commit_exists(source_repo, source_sha):
        raise SystemExit(f"unknown source SHA {source_sha[:12]} in {source_repo}")
    code, out = _git(source_repo, "ls-tree", "-r", "-z", source_sha)
    if code != 0:
        raise SystemExit("cannot list the source snapshot")
    files = []
    for entry in out.split(b"\0"):
        if not entry:
            continue
        meta, path = entry.split(b"\t", 1)
        mode, kind, oid = meta.split(b" ")
        if kind != b"blob":
            continue
        code, data = _git(source_repo, "cat-file", "blob", oid.decode())
        if code != 0:
            raise SystemExit(f"cannot read blob for {path.decode()}")
        files.append({"path": path.decode(), "mode": mode.decode(), "sha256": raw_digest(data)})
    files.sort(key=lambda item: item["path"])
    manifest: dict[str, Any] = {
        "schema_version": 1,
        "kind": MANIFEST_KIND,
        "source_repository": source_repository,
        "source_sha": source_sha,
        "destination": destination,
        "digest": "sha256 of the exact blob bytes at source_sha",
        "file_count": len(files),
        "files": files,
        "manifest_digest": "",
    }
    manifest["manifest_digest"] = canonical_digest(manifest, "manifest_digest")
    return manifest


def build_verification(
    bundle: dict[str, Any],
    bundle_relpath: str,
    source_repo: Path,
    source_repository: str,
    source_sha: str,
    manifest: dict[str, Any],
) -> dict[str, Any]:
    """Verify the reviewed commit against the original history and record it."""
    prov = bundle["implementation_provenance"]
    reviewed = prov["reviewed_commit"]
    if not commit_exists(source_repo, reviewed):
        raise SystemExit(f"reviewed_commit {reviewed[:12]} is not in the source repository history")
    code, _ = _git(source_repo, "merge-base", "--is-ancestor", reviewed, source_sha)
    if code != 0:
        raise SystemExit(f"reviewed_commit {reviewed[:12]} is not an ancestor of {source_sha[:12]}")
    components = []
    for component in prov["components"]:
        at_reviewed = blob_digest(source_repo, reviewed, component["path"])
        at_source = blob_digest(source_repo, source_sha, component["path"])
        if at_reviewed != component["blob_sha256"] or at_source != component["blob_sha256"]:
            raise SystemExit(f"component {component['path']} differs between reviewed_commit and source SHA")
        components.append(
            {
                "path": component["path"],
                "blob_sha256_at_reviewed_commit": at_reviewed,
                "blob_sha256_at_source_sha": at_source,
            }
        )
    record: dict[str, Any] = {
        "schema_version": 1,
        "kind": VERIFICATION_KIND,
        "is_original_approval": False,
        "scope": VERIFICATION_SCOPE,
        "original_approval": {
            "repository": prov["repository"],
            "reviewed_commit": reviewed,
            "evidence": bundle_relpath,
            "bundle_id": bundle["bundle_id"],
        },
        "source": {"repository": source_repository, "sha": source_sha},
        "ancestry": {
            "reviewed_commit_is_ancestor_of_source_sha": True,
            "verified_against": f"{source_repository} Git history",
        },
        "components": components,
        "manifest_digest": manifest["manifest_digest"],
        "verification_digest": "",
    }
    record["verification_digest"] = canonical_digest(record, "verification_digest")
    return record


# ---------------------------------------------------------------------------
# Verification


def migration_provenance_errors(
    bundle: dict[str, Any],
    repo: Path,
    *,
    head: str,
    source_repo: Path | None = None,
    bundle_path: str | None = None,
) -> list[str]:
    context, errs = load_migration(repo)
    if context is None:
        return errs
    entry, manifest, verification = context["entry"], context["manifest"], context["verification"]
    source_repository = entry.get("source", "")
    source_sha = entry.get("sha", "")
    if not _SHA_RE.match(source_sha):
        errs.append(f"unknown source SHA for {context['destination']} in migration/sources.json")

    prov = bundle.get("implementation_provenance", {})
    reviewed = prov.get("reviewed_commit", "")
    bundle_components = {c.get("path", ""): c.get("blob_sha256", "") for c in prov.get("components", [])}

    # The original approval is preserved, never rewritten.
    if bundle.get("integrations", {}).get("commit") != reviewed:
        errs.append("integrations.commit must equal implementation_provenance.reviewed_commit")
    if prov.get("repository") != source_repository:
        errs.append(
            "implementation_provenance.repository does not match the migration source "
            f"repository {source_repository}"
        )

    if manifest is not None:
        if manifest.get("kind") != MANIFEST_KIND:
            errs.append("migration manifest has the wrong kind")
        if manifest.get("manifest_digest") != canonical_digest(manifest, "manifest_digest"):
            errs.append("migration manifest digest mismatch (manifest altered)")
        if manifest.get("source_sha") != source_sha:
            errs.append("unknown source SHA: migration manifest does not match migration/sources.json")
        if manifest.get("source_repository") != source_repository:
            errs.append("migration manifest source repository does not match migration/sources.json")
        if manifest.get("destination") != context["destination"]:
            errs.append("migration manifest destination does not match this component")

    if verification is not None:
        if verification.get("kind") != VERIFICATION_KIND:
            errs.append("migration verification has the wrong kind")
        if verification.get("is_original_approval") is not False:
            errs.append("migration verification must not claim to be an original approval")
        if verification.get("verification_digest") != canonical_digest(verification, "verification_digest"):
            errs.append("migration verification digest mismatch (verification altered)")
        original = verification.get("original_approval", {})
        if original.get("reviewed_commit") != reviewed:
            errs.append(
                "implementation_provenance.reviewed_commit is not the commit the migration "
                "verification covers (altered provenance)"
            )
        if original.get("repository") != prov.get("repository"):
            errs.append("migration verification covers a different original repository")
        if verification.get("scope") != VERIFICATION_SCOPE:
            errs.append("migration verification scope is not the migration-only scope")
        expected_against = f"{source_repository} Git history"
        if verification.get("ancestry", {}).get("verified_against") != expected_against:
            errs.append("migration verification does not record verification against the source history")
        if bundle_path is not None and original.get("evidence") != bundle_path:
            errs.append(
                f"migration verification covers different evidence ({original.get('evidence')!r}, "
                f"not {bundle_path!r})"
            )
        if original.get("bundle_id") != bundle.get("bundle_id"):
            errs.append("migration verification covers a different bundle (bundle_id mismatch)")
        if verification.get("source", {}).get("sha") != source_sha:
            errs.append("unknown source SHA: migration verification does not match migration/sources.json")
        if manifest is not None and verification.get("manifest_digest") != manifest.get("manifest_digest"):
            errs.append("migration verification does not bind the migration manifest")
        if verification.get("ancestry", {}).get("reviewed_commit_is_ancestor_of_source_sha") is not True:
            errs.append("migration verification does not establish reviewed_commit ancestry")

    verified = {
        c.get("path", ""): c for c in (verification or {}).get("components", []) if isinstance(c, dict)
    }
    manifest_files = {f.get("path", ""): f.get("sha256", "") for f in (manifest or {}).get("files", [])}
    for path, digest in bundle_components.items():
        record = verified.get(path)
        if record is None:
            errs.append(f"implementation path absent at reviewed_commit per migration verification: {path}")
        else:
            if record.get("blob_sha256_at_reviewed_commit") != digest:
                errs.append(f"implementation blob digest stale at reviewed_commit: {path}")
            if record.get("blob_sha256_at_source_sha") != digest:
                errs.append(f"implementation changed between reviewed_commit and the imported snapshot: {path}")
        current = repo / path
        if path not in manifest_files:
            errs.append(f"implementation path absent from the imported snapshot manifest: {path}")
        elif not current.is_file() or raw_digest(current.read_bytes()) != manifest_files[path]:
            errs.append(f"implementation changed after import (snapshot content mismatch): {path}")
        if not current.is_file() or validator.file_digest(current) != digest:
            errs.append(f"implementation changed after reviewed_commit (bundle describes stale behavior): {path}")
    for path in sorted(set(verified) - set(bundle_components)):
        errs.append(f"migration verification lists a component the bundle does not: {path}")

    if source_repo is not None:
        errs.extend(_source_history_errors(bundle_components, reviewed, source_sha, manifest, source_repo))

    # Remaining checks of the original provenance step, unchanged in meaning.
    for stage in bundle.get("stages", []):
        transformation = stage.get("transformation")
        if stage.get("status") == "passed" and isinstance(transformation, dict):
            impl_path = transformation.get("implementation", {}).get("path", "")
            if impl_path and impl_path not in bundle_components:
                errs.append(
                    f"stage {stage.get('stage_id')} cites implementation {impl_path} "
                    "not registered in implementation_provenance.components"
                )
    exact = prov.get("exact_head_validation")
    if isinstance(exact, dict) and exact.get("head_commit") != head:
        errs.append("exact_head_validation.head_commit does not match the current head")
    return errs


def _source_history_errors(
    components: dict[str, str],
    reviewed: str,
    source_sha: str,
    manifest: dict[str, Any] | None,
    source_repo: Path,
) -> list[str]:
    """Re-verify against the original repository's history when it is available."""
    errs: list[str] = []
    if not commit_exists(source_repo, source_sha):
        return [f"unknown source SHA {source_sha[:12]}: not in the source repository history"]
    if not commit_exists(source_repo, reviewed):
        return [f"reviewed_commit {reviewed[:12]} is not in the source repository history"]
    code, _ = _git(source_repo, "merge-base", "--is-ancestor", reviewed, source_sha)
    if code != 0:
        errs.append(
            f"implementation_provenance.reviewed_commit {reviewed[:12]} is not an ancestor "
            f"of the imported source SHA {source_sha[:12]}"
        )
    for path, digest in components.items():
        if blob_digest(source_repo, reviewed, path) != digest:
            errs.append(f"source history: blob at reviewed_commit does not match the bundle: {path}")
        if blob_digest(source_repo, source_sha, path) != digest:
            errs.append(f"source history: blob at the imported SHA does not match the bundle: {path}")
    if manifest is not None:
        rebuilt = build_manifest(
            source_repo, manifest.get("source_repository", ""), source_sha, manifest.get("destination", "")
        )
        if rebuilt["manifest_digest"] != manifest.get("manifest_digest"):
            errs.append("source history: migration manifest does not match the source snapshot")
    return errs


def _resolve_source_repo(explicit: Path | None) -> Path | None:
    if explicit is not None:
        return explicit
    env = os.environ.get(SOURCE_ENV)
    return Path(env) if env else None


def provenance_errors(
    bundle: dict[str, Any],
    repo: Path = _REPO,
    *,
    head: str | None = None,
    source_repo: Path | None = None,
    bundle_path: str | None = None,
) -> list[str]:
    """Original provenance in the standalone source repository; migration
    provenance inside the monorepo."""
    if head is None:
        code, out = _git(repo, "rev-parse", "HEAD")
        head = out.decode().strip() if code == 0 else ""
    if locate(repo) is None:
        # Standalone original repository: its own history is the authority.
        return validator.provenance_errors(bundle, repo, head=head)
    # Inside the monorepo, always use migration evidence. A commit that happens to
    # exist in the monorepo history is never the original review.
    return migration_provenance_errors(
        bundle,
        repo,
        head=head,
        source_repo=_resolve_source_repo(source_repo),
        bundle_path=bundle_path,
    )


def semantic_errors(
    bundle: dict[str, Any],
    repo: Path = _REPO,
    *,
    head: str | None = None,
    source_repo: Path | None = None,
    bundle_path: str | None = None,
) -> list[str]:
    """The validator's semantic checks with only the provenance step replaced."""
    if head is None:
        code, out = _git(repo, "rev-parse", "HEAD")
        head = out.decode().strip() if code == 0 else ""
    remaining = Counter(validator.semantic_errors(bundle, repo, head=head))
    remaining.subtract(Counter(validator.provenance_errors(bundle, repo, head=head)))
    errs = [error for error, count in remaining.items() for _ in range(max(count, 0))]
    return errs + provenance_errors(
        bundle, repo, head=head, source_repo=source_repo, bundle_path=bundle_path
    )


def validate_bundle(
    path: Path, *, repo: Path = _REPO, head: str | None = None, source_repo: Path | None = None
) -> list[str]:
    bundle = json.loads(path.read_text(encoding="utf-8"))
    schema = json.loads(validator._SCHEMA.read_text(encoding="utf-8"))
    errs = validator._check(bundle, schema, "bundle")
    if errs:
        return errs
    try:
        relpath: str | None = path.resolve().relative_to(repo.resolve()).as_posix()
    except ValueError:
        relpath = None
    return semantic_errors(bundle, repo, head=head, source_repo=source_repo, bundle_path=relpath)


def reviewed_blob_digest(repo: Path, commit: str, path: str) -> str | None:
    """Digest of ``path`` at the reviewed ``commit``: from local history when it
    resolves, otherwise from the migration verification that covers ``commit``."""
    if commit_exists(repo, commit):
        return blob_digest(repo, commit, path)
    context, errs = load_migration(repo)
    if context is None or errs or context["verification"] is None:
        return None
    verification = context["verification"]
    if verification.get("original_approval", {}).get("reviewed_commit") != commit:
        return None
    for component in verification.get("components", []):
        if component.get("path") == path:
            digest = component.get("blob_sha256_at_reviewed_commit")
            return digest if isinstance(digest, str) else None
    return None


def emit_head_receipt(bundle: dict[str, Any], repo: Path, out_path: Path) -> dict[str, Any]:
    """The validator's exact-head receipt, with component paths resolved relative
    to the component directory so it also works inside the monorepo."""
    from datetime import datetime, timezone

    code, out = _git(repo, "rev-parse", "HEAD")
    head = out.decode().strip()
    components = [
        {"path": c["path"], "blob_sha256": blob_digest(repo, head, c["path"]) or ""}
        for c in bundle.get("implementation_provenance", {}).get("components", [])
    ]
    receipt: dict[str, Any] = {
        "schema_version": 1,
        "kind": "information-cycle-exact-head-validation-receipt",
        "bundle_id": bundle.get("bundle_id", ""),
        "head_commit": head,
        "reviewed_commit": bundle.get("implementation_provenance", {}).get("reviewed_commit", ""),
        "components": components,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "receipt_id": "",
    }
    unsigned = {k: v for k, v in receipt.items() if k not in ("receipt_id", "generated_at")}
    receipt["receipt_id"] = "sha256:" + sha256(
        json.dumps(unsigned, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return receipt


# ---------------------------------------------------------------------------
# Command line


def _write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _cmd_validate(args: argparse.Namespace) -> int:
    path = Path(args.bundle)
    source_repo = _resolve_source_repo(args.source_repo)
    errs = validate_bundle(path, source_repo=source_repo)
    if errs:
        print("information-cycle bundle validation FAILED:", file=sys.stderr)
        for err in errs:
            print(f"- {err}", file=sys.stderr)
        return 1
    bundle = json.loads(path.read_text(encoding="utf-8"))
    if locate(_REPO) is None:
        mode = "original history"
    elif source_repo is not None:
        mode = "migration evidence, re-verified against source history"
    else:
        mode = "migration evidence (source history not available locally)"
    proven = sum(1 for s in bundle["stages"] if s["status"] == "passed")
    print(
        f"information-cycle bundle OK: {proven}/{len(bundle['stages'])} stages "
        f"component-proven, {len(bundle['unproven_downstream'])} honestly unproven; "
        f"provenance verified by {mode}"
    )
    if args.emit_head_receipt:
        if args.expected_head:
            guard = validator.assert_exact_head(_REPO, args.expected_head)
            if guard:
                for err in guard:
                    print(f"- {err}", file=sys.stderr)
                return 1
        receipt = emit_head_receipt(bundle, _REPO, Path(args.emit_head_receipt))
        if args.expected_head and receipt["head_commit"] != args.expected_head:
            print("- emitted receipt does not bind the expected head", file=sys.stderr)
            return 1
        print(f"exact-head validation receipt: {receipt['receipt_id']} (head {receipt['head_commit'][:12]})")
    return 0


def _cmd_write_records(args: argparse.Namespace) -> int:
    context, _ = load_migration(_REPO)
    if context is None:
        print("not a monorepo component with a migration record", file=sys.stderr)
        return 1
    entry = context["entry"]
    bundle_path = Path(args.bundle).resolve()
    bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
    manifest = build_manifest(args.source_repo, entry["source"], entry["sha"], context["destination"])
    verification = build_verification(
        bundle,
        bundle_path.relative_to(_REPO.resolve()).as_posix(),
        args.source_repo,
        entry["source"],
        entry["sha"],
        manifest,
    )
    _write_json(manifest_path(context["root"], entry["source"]), manifest)
    _write_json(verification_path(context["root"], entry["source"]), verification)
    print(f"wrote manifest ({manifest['file_count']} files) and migration verification")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    validate = sub.add_parser("validate", help="validate a bundle with migration-aware provenance")
    validate.add_argument("bundle")
    validate.add_argument("--source-repo", type=Path, default=None)
    validate.add_argument("--emit-head-receipt", default=None)
    validate.add_argument("--expected-head", default=None)
    write = sub.add_parser("write-records", help="generate migration records from the source history")
    write.add_argument("bundle")
    write.add_argument("--source-repo", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.command == "validate":
        return _cmd_validate(args)
    return _cmd_write_records(args)


if __name__ == "__main__":
    raise SystemExit(main())
