#!/usr/bin/env python3
"""Compile the public Bot manifest from one exact release publication binding."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import re
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urlparse

SHA = re.compile(r"^[a-f0-9]{40}$")
HEX_DIGEST = re.compile(r"^[a-f0-9]{64}$")
PREFIXED_DIGEST = re.compile(r"^sha256:[a-f0-9]{64}$")
SEMVER = re.compile(
    r"^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:[-+][0-9A-Za-z.-]+)?$"
)
EXPECTED_TARGETS = frozenset(
    {
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    }
)
BINDING_KEYS = {
    "schema_version",
    "release_id",
    "release_manifest_digest",
    "bot_commit",
    "bot_version",
    "mcp",
    "binding_digest",
}
MCP_KEYS = {
    "package",
    "commit",
    "version",
    "wheel_url",
    "wheel_sha256",
    "release_descriptor_digest",
    "tool_request_protocol",
    "python_requires",
}


def canonical_digest(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def load_binding(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("release publication binding must be a JSON object")
    return value


def _require_timestamp(value: str) -> None:
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValueError("released_at must be an offset-aware date-time") from exc
    if parsed.tzinfo is None:
        raise ValueError("released_at must be an offset-aware date-time")


def validate_binding(
    binding: dict[str, Any],
    *,
    tag: str,
    bot_commit: str,
    bot_version: str,
) -> dict[str, Any]:
    if set(binding) != BINDING_KEYS:
        raise ValueError(f"release binding keys must be exactly {sorted(BINDING_KEYS)}")
    if binding.get("schema_version") != 1:
        raise ValueError("release binding schema_version must be 1")
    if not SHA.fullmatch(bot_commit):
        raise ValueError("Bot commit must be a full lowercase Git SHA")
    if binding.get("bot_commit") != bot_commit:
        raise ValueError("Bot commit does not match the release publication binding")
    if not SEMVER.fullmatch(bot_version):
        raise ValueError("Bot version must be semantic")
    if binding.get("bot_version") != bot_version:
        raise ValueError("Bot version does not match the release publication binding")
    if tag != f"v{bot_version}":
        raise ValueError("release tag does not match the Bot version")
    if not PREFIXED_DIGEST.fullmatch(str(binding.get("release_id", ""))):
        raise ValueError("release_id must be a sha256 digest")
    if not PREFIXED_DIGEST.fullmatch(str(binding.get("release_manifest_digest", ""))):
        raise ValueError("release_manifest_digest must be a sha256 digest")

    mcp = binding.get("mcp")
    if not isinstance(mcp, dict) or set(mcp) != MCP_KEYS:
        raise ValueError(f"MCP binding keys must be exactly {sorted(MCP_KEYS)}")
    if mcp.get("package") != "bicameral-mcp":
        raise ValueError("MCP package must be bicameral-mcp")
    if not SHA.fullmatch(str(mcp.get("commit", ""))):
        raise ValueError("MCP commit must be a full lowercase Git SHA")
    mcp_version = str(mcp.get("version", ""))
    if not SEMVER.fullmatch(mcp_version):
        raise ValueError("MCP version must be semantic")
    wheel_url = str(mcp.get("wheel_url", ""))
    parsed_wheel = urlparse(wheel_url)
    if parsed_wheel.scheme != "https" or not parsed_wheel.netloc:
        raise ValueError("MCP wheel URL must be absolute HTTPS")
    wheel_filename = unquote(Path(parsed_wheel.path).name)
    if not re.fullmatch(
        rf"bicameral_mcp-{re.escape(mcp_version)}-[A-Za-z0-9_.-]+\.whl",
        wheel_filename,
    ):
        raise ValueError("MCP wheel filename must bind the exact package version")
    if not HEX_DIGEST.fullmatch(str(mcp.get("wheel_sha256", ""))):
        raise ValueError("MCP wheel SHA-256 must be 64 lowercase hexadecimal characters")
    if not PREFIXED_DIGEST.fullmatch(str(mcp.get("release_descriptor_digest", ""))):
        raise ValueError("MCP release descriptor digest must use sha256:")
    if mcp.get("tool_request_protocol") != "v2":
        raise ValueError("MCP ToolRequest protocol must be v2")
    if not str(mcp.get("python_requires", "")).startswith(">=3.11"):
        raise ValueError("MCP Python requirement must require Python 3.11 or newer")

    unsigned = {key: value for key, value in binding.items() if key != "binding_digest"}
    if binding.get("binding_digest") != canonical_digest(unsigned):
        raise ValueError("release binding digest does not bind the complete payload")
    return mcp


def _compile_assets(release_dir: Path, *, base_url: str, tag: str) -> dict[str, Any]:
    archives = sorted(release_dir.glob("bicameral-*.tar.gz"))
    targets = {
        archive.name.removeprefix("bicameral-").removesuffix(".tar.gz")
        for archive in archives
    }
    if targets != EXPECTED_TARGETS or len(archives) != len(EXPECTED_TARGETS):
        raise ValueError(
            "release asset matrix must contain exactly "
            + ", ".join(sorted(EXPECTED_TARGETS))
        )
    assets: dict[str, Any] = {}
    for archive in archives:
        target = archive.name.removeprefix("bicameral-").removesuffix(".tar.gz")
        assets[target] = {
            "name": archive.name,
            "url": f"{base_url}/{tag}/{archive.name}",
            "sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        }
    return dict(sorted(assets.items()))


def compile_manifest(
    *,
    binding: dict[str, Any],
    release_dir: Path,
    tag: str,
    bot_commit: str,
    bot_version: str,
    download_base_url: str,
    released_at: str,
) -> dict[str, Any]:
    _require_timestamp(released_at)
    parsed_base = urlparse(download_base_url)
    if parsed_base.scheme != "https" or not parsed_base.netloc:
        raise ValueError("download base URL must be absolute HTTPS")
    base_url = download_base_url.rstrip("/")
    mcp = validate_binding(
        binding,
        tag=tag,
        bot_commit=bot_commit,
        bot_version=bot_version,
    )
    public_mcp = {key: mcp[key] for key in MCP_KEYS if key != "commit"}
    return {
        "schema_version": 2,
        "version": bot_version,
        "tag": tag,
        "bot_commit": bot_commit,
        "mcp_commit": mcp["commit"],
        "release_id": binding["release_id"],
        "release_manifest_digest": binding["release_manifest_digest"],
        "binding_digest": binding["binding_digest"],
        "released_at": released_at,
        "checksums_url": f"{base_url}/{tag}/SHA256SUMS",
        "assets": _compile_assets(release_dir, base_url=base_url, tag=tag),
        "mcp": public_mcp,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binding", type=Path, required=True)
    parser.add_argument("--release-dir", type=Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--bot-commit", required=True)
    parser.add_argument("--bot-version", required=True)
    parser.add_argument("--download-base-url")
    parser.add_argument("--released-at")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    binding = load_binding(args.binding)
    if args.validate_only:
        validate_binding(
            binding,
            tag=args.tag,
            bot_commit=args.bot_commit,
            bot_version=args.bot_version,
        )
        print(args.binding)
        return 0
    if args.release_dir is None or args.download_base_url is None or args.output is None:
        parser.error(
            "--release-dir, --download-base-url, and --output are required "
            "unless --validate-only is used"
        )
    released_at = args.released_at or (
        datetime.datetime.now(datetime.UTC)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )
    manifest = compile_manifest(
        binding=binding,
        release_dir=args.release_dir,
        tag=args.tag,
        bot_commit=args.bot_commit,
        bot_version=args.bot_version,
        download_base_url=args.download_base_url,
        released_at=released_at,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
