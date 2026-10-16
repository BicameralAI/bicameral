#!/usr/bin/env python3
"""Enforce live, node-specific Figma references for product UI specifications."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


RASTER_SUFFIXES = {".gif", ".jpeg", ".jpg", ".png", ".webp"}
FIGMA_URL = re.compile(r"https://(?:www\.)?figma\.com/design/[^\s)>]+")
FIGMA_REFERENCE = re.compile(r"\bfigma\b", re.IGNORECASE)
FIGMA_MARKDOWN_LINK = re.compile(
    r"\[[^\]]+\]\(https://(?:www\.)?figma\.com/design/[^\s)>]*node-id=[^\s)>]+\)",
    re.IGNORECASE,
)


def checked_in_spec_rasters(root: Path) -> list[Path]:
    assets = root / "docs/specs/assets"
    if not assets.exists():
        return []
    return sorted(
        path.relative_to(root)
        for path in assets.rglob("*")
        if path.is_file() and path.suffix.lower() in RASTER_SUFFIXES
    )


def non_specific_figma_links(root: Path) -> list[str]:
    violations: list[str] = []
    docs = root / "docs"
    if not docs.exists():
        return violations
    for path in sorted(docs.rglob("*.md")):
        for line_number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            for url in FIGMA_URL.findall(line):
                if "node-id=" not in url:
                    violations.append(f"{path.relative_to(root)}:{line_number}")
    return violations


def unlinked_figma_references(root: Path) -> list[str]:
    """Return prose/table references to Figma without a live node link nearby."""

    violations: list[str] = []
    docs = root / "docs"
    if not docs.exists():
        return violations

    for path in sorted(docs.rglob("*.md")):
        lines = path.read_text(encoding="utf-8").splitlines()
        blocks: list[tuple[int, str]] = []
        paragraph: list[str] = []
        paragraph_start = 1
        in_fence = False

        def flush_paragraph() -> None:
            nonlocal paragraph
            if paragraph:
                blocks.append((paragraph_start, "\n".join(paragraph)))
                paragraph = []

        for line_number, line in enumerate(lines, start=1):
            if line.lstrip().startswith("```"):
                flush_paragraph()
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            if not line.strip():
                flush_paragraph()
                continue
            if line.lstrip().startswith("|"):
                flush_paragraph()
                blocks.append((line_number, line))
                continue
            if not paragraph:
                paragraph_start = line_number
            paragraph.append(line)
        flush_paragraph()

        for line_number, block in blocks:
            if FIGMA_REFERENCE.search(block) and not FIGMA_MARKDOWN_LINK.search(block):
                violations.append(f"{path.relative_to(root)}:{line_number}")
    return violations


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()

    root = args.root.resolve()
    raster_violations = checked_in_spec_rasters(root)
    link_violations = non_specific_figma_links(root)
    reference_violations = unlinked_figma_references(root)
    if raster_violations:
        print(
            "Checked-in UI raster evidence is not allowed. "
            "Link to the live Figma node instead:",
            file=sys.stderr,
        )
        for path in raster_violations:
            print(f"- {path}", file=sys.stderr)
    if link_violations:
        print(
            "Figma references must use a live, node-specific design URL:",
            file=sys.stderr,
        )
        for location in link_violations:
            print(f"- {location}", file=sys.stderr)
    if reference_violations:
        print(
            "Figma references must be Markdown links to live, node-specific designs:",
            file=sys.stderr,
        )
        for location in reference_violations:
            print(f"- {location}", file=sys.stderr)
    return 1 if raster_violations or link_violations or reference_violations else 0


if __name__ == "__main__":
    raise SystemExit(main())
