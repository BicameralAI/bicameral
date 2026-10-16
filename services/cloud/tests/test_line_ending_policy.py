"""Assert repository LF policy for every input to a fail-closed digest."""

from __future__ import annotations

import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
CONTRACTS = REPO / "src" / "bicameral_cloud" / "contracts"
ONBOARDING = CONTRACTS / "onboarding"


def _git(*args: str, stdin: bytes | None = None) -> bytes:
    return subprocess.run(
        ["git", *args],
        cwd=REPO,
        check=True,
        capture_output=True,
        input=stdin,
    ).stdout


def _digest_bearing_paths() -> list[str]:
    paths = [
        CONTRACTS / "hosted_bridge_contract.json",
        ONBOARDING / "source.json",
        *sorted(ONBOARDING.glob("schemas/*.schema.json")),
        *sorted(ONBOARDING.glob("fixtures/*.json")),
    ]
    return [path.relative_to(REPO).as_posix() for path in paths]


def _eol_attributes(paths: list[str]) -> dict[str, str]:
    payload = ("\n".join(paths) + "\n").encode()
    output = _git("check-attr", "eol", "--stdin", stdin=payload).decode()
    resolved: dict[str, str] = {}
    for line in output.splitlines():
        if not line:
            continue
        path, _, value = line.rsplit(": ", 2)
        resolved[path] = value
    return resolved


def _summarize(offenders: dict[str, str], limit: int = 5) -> str:
    sample = list(offenders.items())[:limit]
    rendered = ", ".join(f"{path} -> {value}" for path, value in sample)
    if len(offenders) > limit:
        rendered += f", and {len(offenders) - limit} more"
    return f"{len(offenders)} path(s): {rendered}"


def test_digest_bearing_files_resolve_to_lf() -> None:
    paths = _digest_bearing_paths()
    resolved = _eol_attributes(paths)
    assert paths, "expected at least one fingerprint input"
    unpinned = {path: value for path, value in resolved.items() if value != "lf"}
    assert not unpinned, f"digest inputs not pinned to LF: {_summarize(unpinned)}"


def test_no_tracked_file_is_exempted_from_lf() -> None:
    tracked = _git("ls-files").decode().splitlines()
    resolved = _eol_attributes(tracked)
    exempted = {path: value for path, value in resolved.items() if value != "lf"}
    assert not exempted, f"exempted from LF policy: {_summarize(exempted)}"


def test_no_committed_blob_contains_crlf() -> None:
    tracked = _git("ls-files", "-z").decode().split("\0")
    names = [item for item in tracked if item]
    batch = ("\n".join(f"HEAD:./{name}" for name in names) + "\n").encode()
    output = _git("cat-file", "--batch", stdin=batch)

    offending: list[str] = []
    cursor = 0
    for name in names:
        header_end = output.index(b"\n", cursor)
        size = int(output[cursor:header_end].split()[-1])
        body_start = header_end + 1
        if b"\r\n" in output[body_start : body_start + size]:
            offending.append(name)
        cursor = body_start + size + 1

    assert not offending, f"committed blobs containing CRLF: {offending}"
