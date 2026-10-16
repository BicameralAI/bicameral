#!/usr/bin/env python3
"""Validate architecture records and generate the canonical architecture index.

Records live in the canonical directory named by governance/structure.json
(``canonical.architecture_records``) as ``ARCH-NNNN-slug.md``. This checker
validates each record, enforces the authority rules across all records, and
generates docs/architecture/INDEX.md and docs/architecture/index.json.

Usage:
    python3 tools/governance/check_architecture.py          # validate only
    python3 tools/governance/check_architecture.py --write  # regenerate, then validate

Standard library only (Python 3.10+).
"""

from __future__ import annotations

import argparse
import datetime
import json
import posixpath
import re
import sys
from dataclasses import dataclass
from pathlib import Path

MANIFEST_PATH = "governance/structure.json"

FIELDS = (
    "id",
    "title",
    "kind",
    "status",
    "date",
    "owners",
    "governs",
    "depends_on",
    "supersedes",
    "references",
)
SCALAR_FIELDS = ("id", "title", "kind", "status", "date")
LIST_FIELDS = ("owners", "governs", "depends_on", "supersedes", "references")
RELATIONSHIP_FIELDS = ("depends_on", "supersedes", "references")
DERIVED_FIELDS = ("required_by", "superseded_by", "proposed_replacements")

KINDS = ("decision", "design", "contract", "reference")
STATUSES = ("proposed", "accepted", "superseded", "deprecated", "rejected")
HISTORICAL = ("superseded", "deprecated", "rejected")
# Statuses whose supersession of a predecessor actually took effect at some point.
EFFECTIVE_REPLACEMENT = ("accepted", "superseded", "deprecated")

REQUIRED_SECTIONS = {
    "decision": ("Context", "Decision", "Consequences"),
    "design": ("Context", "Design"),
    "contract": ("Scope", "Contract"),
    "reference": (),
}

ID_PATTERN = re.compile(r"^ARCH-\d{4}$")
ID_IN_TEXT = re.compile(r"\bARCH-\d{4}\b")
FILENAME_PATTERN = re.compile(r"^(ARCH-\d{4})-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$")
TOPIC_PATTERN = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
DATE_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}$")
OWNER_PATTERN = re.compile(
    r"^@[A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38}(?:/[A-Za-z0-9][A-Za-z0-9._-]*)?$"
)
FRONT_MATTER_LINE = re.compile(r"^([a-z_]+): ?(.*)$")
LIST_ITEM_PATTERN = re.compile(r"^[^\s,\[\]][^,\[\]]*$")

INDEX_MD = "INDEX.md"
INDEX_JSON = "index.json"


@dataclass
class Record:
    path: str
    id: str
    title: str
    kind: str
    status: str
    date: str
    owners: list[str]
    governs: list[str]
    depends_on: list[str]
    supersedes: list[str]
    references: list[str]
    body: str

    @property
    def arch_relationships(self) -> set[str]:
        related = set(self.depends_on) | set(self.supersedes)
        related |= {item for item in self.references if ID_PATTERN.match(item)}
        return related


# ---------------------------------------------------------------------------
# Parsing


def parse_front_matter(text: str, where: str) -> tuple[dict[str, object], str, list[str]]:
    """Parse the restricted front matter: ``key: value`` and ``key: [a, b]`` only."""
    errors: list[str] = []
    lines = text.split("\n")
    if not lines or lines[0] != "---":
        return {}, text, [f"{where}: must start with '---' front matter"]
    try:
        end = lines.index("---", 1)
    except ValueError:
        return {}, text, [f"{where}: front matter is not terminated with '---'"]
    fields: dict[str, object] = {}
    for number, line in enumerate(lines[1:end], start=2):
        match = FRONT_MATTER_LINE.match(line)
        if not match:
            errors.append(f"{where}:{number}: unsupported front matter line {line!r}")
            continue
        key, raw = match.group(1), match.group(2).strip()
        if key in fields:
            errors.append(f"{where}:{number}: duplicate key {key!r}")
            continue
        if raw.startswith("[") or raw.endswith("]"):
            if not (raw.startswith("[") and raw.endswith("]")):
                errors.append(f"{where}:{number}: malformed list for {key!r}")
                continue
            inner = raw[1:-1].strip()
            items = [item.strip() for item in inner.split(",")] if inner else []
            bad = [item for item in items if not LIST_ITEM_PATTERN.match(item)]
            if bad:
                errors.append(f"{where}:{number}: invalid list item(s) {bad} for {key!r}")
                continue
            fields[key] = items
        else:
            if not raw:
                errors.append(f"{where}:{number}: empty value for {key!r}")
                continue
            fields[key] = raw
    body = "\n".join(lines[end + 1 :])
    return fields, body, errors


def parse_record(path: str, text: str) -> tuple[Record | None, list[str]]:
    where = path
    if "\r" in text:
        return None, [f"{where}: must use LF line endings"]
    fields, body, errors = parse_front_matter(text, where)
    if not fields and errors:
        return None, errors

    for key in DERIVED_FIELDS:
        if key in fields:
            errors.append(f"{where}: {key!r} is derived and generated into the index; do not author it")
    unknown = sorted(set(fields) - set(FIELDS) - set(DERIVED_FIELDS))
    if unknown:
        errors.append(f"{where}: unsupported front matter keys {unknown}")
    missing = [key for key in FIELDS if key not in fields]
    if missing:
        errors.append(f"{where}: missing front matter keys {missing}")
        return None, errors

    for key in SCALAR_FIELDS:
        if not isinstance(fields[key], str):
            errors.append(f"{where}: {key} must be a scalar value")
    for key in LIST_FIELDS:
        if not isinstance(fields[key], list):
            errors.append(f"{where}: {key} must be a list like [a, b] (use [] for none)")
    if errors:
        return None, errors

    record = Record(
        path=path,
        id=fields["id"],
        title=fields["title"],
        kind=fields["kind"],
        status=fields["status"],
        date=fields["date"],
        owners=fields["owners"],
        governs=fields["governs"],
        depends_on=fields["depends_on"],
        supersedes=fields["supersedes"],
        references=fields["references"],
        body=body,
    )

    if not ID_PATTERN.match(record.id):
        errors.append(f"{where}: id {record.id!r} must match ARCH-NNNN")
    name = posixpath.basename(path)
    match = FILENAME_PATTERN.match(name)
    if not match:
        errors.append(f"{where}: filename must match ARCH-NNNN-slug.md (lowercase slug)")
    elif match.group(1) != record.id:
        errors.append(f"{where}: filename id {match.group(1)} does not match id {record.id}")
    if record.kind not in KINDS:
        errors.append(f"{where}: kind {record.kind!r} must be one of {list(KINDS)}")
    if record.status not in STATUSES:
        errors.append(f"{where}: status {record.status!r} must be one of {list(STATUSES)}")
    if not DATE_PATTERN.match(record.date):
        errors.append(f"{where}: date {record.date!r} must be YYYY-MM-DD")
    else:
        try:
            datetime.date.fromisoformat(record.date)
        except ValueError:
            errors.append(f"{where}: date {record.date!r} is not a valid calendar date")
    if not record.owners:
        errors.append(f"{where}: owners must not be empty")
    for owner in record.owners:
        if not OWNER_PATTERN.match(owner):
            errors.append(f"{where}: invalid owner handle {owner!r}")
    for topic in record.governs:
        if not TOPIC_PATTERN.match(topic):
            errors.append(f"{where}: governs topic {topic!r} must be lowercase-kebab-case")
    for key in ("depends_on", "supersedes"):
        for item in getattr(record, key):
            if not ID_PATTERN.match(item):
                errors.append(f"{where}: {key} entry {item!r} must be an ARCH-NNNN id")
    for key in LIST_FIELDS:
        values = getattr(record, key)
        duplicates = sorted({item for item in values if values.count(item) > 1})
        if duplicates:
            errors.append(f"{where}: duplicate {key} entries {duplicates}")

    seen: dict[str, str] = {}
    for key in RELATIONSHIP_FIELDS:
        for item in set(getattr(record, key)):
            if not ID_PATTERN.match(item):
                continue
            if item in seen:
                errors.append(
                    f"{where}: {item} appears in both {seen[item]} and {key}; "
                    f"list each relationship once"
                )
            else:
                seen[item] = key

    errors.extend(_check_body(record))
    return record, errors


def _check_body(record: Record) -> list[str]:
    errors: list[str] = []
    where = record.path
    lines = record.body.split("\n")
    first = next((line for line in lines if line.strip()), "")
    expected = f"# {record.id}: {record.title}"
    if first != expected:
        errors.append(f"{where}: first heading must be {expected!r}")
    headings = {line[3:].strip() for line in lines if line.startswith("## ")}
    for section in REQUIRED_SECTIONS.get(record.kind, ()):
        if section not in headings:
            errors.append(f"{where}: {record.kind} records require a '## {section}' section")
    mentioned = set(ID_IN_TEXT.findall(record.body)) - {record.id}
    undeclared = sorted(mentioned - record.arch_relationships)
    if undeclared:
        errors.append(
            f"{where}: body references {undeclared} without declaring them in "
            f"depends_on, supersedes or references"
        )
    return errors


# ---------------------------------------------------------------------------
# Cross-record authority rules


def _find_cycle(graph: dict[str, list[str]]) -> list[str] | None:
    WHITE, GREY, BLACK = 0, 1, 2
    color = {node: WHITE for node in graph}
    stack: list[str] = []

    def visit(node: str) -> list[str] | None:
        color[node] = GREY
        stack.append(node)
        for target in sorted(graph.get(node, ())):
            if target not in color:
                continue
            if color[target] == GREY:
                return stack[stack.index(target) :] + [target]
            if color[target] == WHITE:
                cycle = visit(target)
                if cycle:
                    return cycle
        stack.pop()
        color[node] = BLACK
        return None

    for node in sorted(graph):
        if color[node] == WHITE:
            cycle = visit(node)
            if cycle:
                return cycle
    return None


def check_authority(records: list[Record]) -> list[str]:
    errors: list[str] = []
    by_id: dict[str, Record] = {}
    for record in records:
        if record.id in by_id:
            errors.append(
                f"{record.path}: duplicate id {record.id} (also {by_id[record.id].path})"
            )
        else:
            by_id[record.id] = record

    for record in by_id.values():
        where = record.path
        for key in RELATIONSHIP_FIELDS:
            for item in getattr(record, key):
                if not ID_PATTERN.match(item):
                    continue
                if item == record.id:
                    errors.append(f"{where}: {key} must not reference the record itself")
                elif item not in by_id:
                    errors.append(f"{where}: {key} references unknown record {item}")

        # Rule 1: accepted records govern something.
        if record.status == "accepted" and not record.governs:
            errors.append(f"{where}: accepted records must govern at least one topic")

        # Rules 4 and 5: dependency status constraints.
        for dep_id in record.depends_on:
            dep = by_id.get(dep_id)
            if dep is None or dep_id == record.id:
                continue
            if record.status == "accepted" and dep.status != "accepted":
                errors.append(
                    f"{where}: accepted record depends on {dep_id}, which is {dep.status}; "
                    f"accepted records may depend only on accepted records"
                )
            if record.status == "proposed" and dep.status in HISTORICAL:
                errors.append(
                    f"{where}: proposed record depends on {dep_id}, which is {dep.status}; "
                    f"depend on its accepted replacement instead"
                )

        # Rules 10 and 11: supersession transitions.
        for old_id in record.supersedes:
            old = by_id.get(old_id)
            if old is None or old_id == record.id:
                continue
            if record.status == "proposed":
                if old.status != "accepted":
                    errors.append(
                        f"{where}: a proposed replacement may only supersede an accepted "
                        f"record; {old_id} is {old.status}"
                    )
            elif record.status in EFFECTIVE_REPLACEMENT:
                if old.status != "superseded":
                    errors.append(
                        f"{where}: {record.status} record supersedes {old_id}, which is "
                        f"{old.status}; mark {old_id} superseded in the same change"
                    )

    # Rule 2: exactly one accepted record per topic.
    owners_by_topic: dict[str, list[str]] = {}
    for record in by_id.values():
        if record.status == "accepted":
            for topic in record.governs:
                owners_by_topic.setdefault(topic, []).append(record.id)
    for topic, ids in sorted(owners_by_topic.items()):
        if len(ids) > 1:
            errors.append(
                f"topic {topic!r} is governed by multiple accepted records {sorted(ids)}"
            )

    # Rule 12: superseded records need a replacement that took effect.
    for record in by_id.values():
        if record.status != "superseded":
            continue
        replacements = [
            other.id
            for other in by_id.values()
            if record.id in other.supersedes and other.status in EFFECTIVE_REPLACEMENT
        ]
        if not replacements:
            errors.append(
                f"{record.path}: superseded records must be superseded by an accepted "
                f"(or later superseded/deprecated) replacement"
            )

    # Rule 6: acyclic dependencies (and supersession chains).
    for key in ("depends_on", "supersedes"):
        graph = {rid: [d for d in getattr(r, key) if d in by_id] for rid, r in by_id.items()}
        cycle = _find_cycle(graph)
        if cycle:
            errors.append(f"{key} cycle: {' -> '.join(cycle)}")

    return errors


# ---------------------------------------------------------------------------
# Index generation


def build_index(records: list[Record]) -> dict[str, object]:
    records = sorted(records, key=lambda r: r.id)
    by_id = {r.id: r for r in records}
    required_by: dict[str, list[str]] = {r.id: [] for r in records}
    superseded_by: dict[str, list[str]] = {r.id: [] for r in records}
    proposed_replacements: dict[str, list[str]] = {r.id: [] for r in records}
    for record in records:
        for dep in record.depends_on:
            if dep in required_by:
                required_by[dep].append(record.id)
        for old in record.supersedes:
            if old not in by_id:
                continue
            if record.status in EFFECTIVE_REPLACEMENT:
                superseded_by[old].append(record.id)
            elif record.status == "proposed":
                proposed_replacements[old].append(record.id)

    authority = {}
    proposed_claims: dict[str, list[str]] = {}
    for record in records:
        for topic in record.governs:
            if record.status == "accepted":
                authority[topic] = record.id
            elif record.status == "proposed":
                proposed_claims.setdefault(topic, []).append(record.id)

    entries = []
    for record in records:
        entries.append(
            {
                "id": record.id,
                "title": record.title,
                "kind": record.kind,
                "status": record.status,
                "date": record.date,
                "path": record.path,
                "owners": sorted(record.owners),
                "governs": sorted(record.governs),
                "depends_on": sorted(record.depends_on),
                "supersedes": sorted(record.supersedes),
                "references": sorted(record.references),
                "required_by": sorted(required_by[record.id]),
                "superseded_by": sorted(superseded_by[record.id]),
                "proposed_replacements": sorted(proposed_replacements[record.id]),
            }
        )

    return {
        "schema_version": 1,
        "generated_by": "tools/governance/check_architecture.py",
        "authority": dict(sorted(authority.items())),
        "proposed_claims": {k: sorted(v) for k, v in sorted(proposed_claims.items())},
        "accepted": [r.id for r in records if r.status == "accepted"],
        "proposed": [r.id for r in records if r.status == "proposed"],
        "historical": [r.id for r in records if r.status in HISTORICAL],
        "records": entries,
    }


def _ids(values: list[str]) -> str:
    return ", ".join(values) if values else "—"


def _escape(text: str) -> str:
    return text.replace("|", "\\|")


def render_index_markdown(index: dict[str, object], records_rel: str) -> str:
    records = {entry["id"]: entry for entry in index["records"]}

    def link(record_id: str) -> str:
        entry = records[record_id]
        return f"[{record_id}]({records_rel}/{posixpath.basename(entry['path'])})"

    out: list[str] = []
    out.append("<!-- Generated by tools/governance/check_architecture.py. Do not edit by hand. -->\n")
    out.append("# Architecture index\n\n")
    out.append(
        "This file is generated from the records in "
        f"[`{records_rel}/`]({records_rel}/). Regenerate it with "
        "`python3 tools/governance/check.py --write`. The machine-readable form is "
        "[`index.json`](index.json).\n\n"
        "Only **accepted** records carry authority. Proposed records express intent; "
        "historical records are retained for traceability.\n\n"
    )

    out.append("## Accepted authority by topic\n\n")
    if index["authority"]:
        out.append("| Topic | Authoritative record |\n| --- | --- |\n")
        for topic, record_id in index["authority"].items():
            out.append(f"| `{topic}` | {link(record_id)} |\n")
    else:
        out.append("_No topic currently has an accepted authoritative record._\n")
    out.append("\n")

    def table(title: str, ids: list[str], empty: str, historical: bool = False) -> None:
        out.append(f"## {title}\n\n")
        if not ids:
            out.append(f"_{empty}_\n\n")
            return
        if historical:
            out.append("| Record | Title | Kind | Status | Date | Topics | Superseded by |\n")
            out.append("| --- | --- | --- | --- | --- | --- | --- |\n")
        else:
            out.append("| Record | Title | Kind | Date | Topics | Depends on | Supersedes |\n")
            out.append("| --- | --- | --- | --- | --- | --- | --- |\n")
        for record_id in ids:
            entry = records[record_id]
            topics = ", ".join(f"`{t}`" for t in entry["governs"]) or "—"
            if historical:
                out.append(
                    f"| {link(record_id)} | {_escape(entry['title'])} | {entry['kind']} | "
                    f"{entry['status']} | {entry['date']} | {topics} | "
                    f"{_ids(entry['superseded_by'])} |\n"
                )
            else:
                out.append(
                    f"| {link(record_id)} | {_escape(entry['title'])} | {entry['kind']} | "
                    f"{entry['date']} | {topics} | {_ids(entry['depends_on'])} | "
                    f"{_ids(entry['supersedes'])} |\n"
                )
        out.append("\n")

    table("Accepted records", index["accepted"], "No accepted records.")
    table(
        "Proposed records",
        index["proposed"],
        "No proposed records.",
    )
    table("Historical records", index["historical"], "No historical records.", historical=True)

    out.append("## Derived relationships\n\n")
    if records:
        out.append("| Record | Required by | Superseded by | Proposed replacements |\n")
        out.append("| --- | --- | --- | --- |\n")
        for record_id, entry in records.items():
            out.append(
                f"| {record_id} | {_ids(entry['required_by'])} | "
                f"{_ids(entry['superseded_by'])} | {_ids(entry['proposed_replacements'])} |\n"
            )
    else:
        out.append("_No records._\n")
    return "".join(out)


def render_index_json(index: dict[str, object]) -> str:
    return json.dumps(index, indent=2, sort_keys=False, ensure_ascii=False) + "\n"


# ---------------------------------------------------------------------------
# Entry points


def records_directory(root: Path) -> str:
    try:
        manifest = json.loads((root / MANIFEST_PATH).read_text(encoding="utf-8"))
        value = manifest["canonical"]["architecture_records"]
    except (OSError, ValueError, KeyError, TypeError) as exc:
        raise ValueError(f"{MANIFEST_PATH}: cannot read canonical.architecture_records ({exc})")
    if not isinstance(value, str) or not value or value.startswith("/") or ".." in value.split("/"):
        raise ValueError(f"{MANIFEST_PATH}: invalid canonical.architecture_records")
    return value


def load_records(root: Path, records_rel: str) -> tuple[list[Record], list[str]]:
    errors: list[str] = []
    records: list[Record] = []
    directory = root / records_rel
    if directory.is_symlink():
        return [], [f"{records_rel}: must not be a symbolic link"]
    if not directory.is_dir():
        return [], [f"{records_rel}: canonical records directory is missing"]
    for path in sorted(directory.iterdir(), key=lambda p: p.name):
        if path.name == "README.md" or path.is_dir():
            continue
        rel = f"{records_rel}/{path.name}"
        if path.suffix != ".md" or not FILENAME_PATTERN.match(path.name):
            errors.append(f"{rel}: only README.md and ARCH-NNNN-slug.md files belong here")
            continue
        if path.is_symlink():
            errors.append(f"{rel}: must not be a symbolic link")
            continue
        record, record_errors = parse_record(rel, path.read_text(encoding="utf-8"))
        errors.extend(record_errors)
        if record is not None:
            records.append(record)
    return records, errors


def generated_outputs(root: Path, records_rel: str, records: list[Record]) -> dict[str, str]:
    index_dir = posixpath.dirname(records_rel)
    index = build_index(records)
    rel_from_index = posixpath.relpath(records_rel, index_dir or ".")
    return {
        posixpath.join(index_dir, INDEX_MD): render_index_markdown(index, rel_from_index),
        posixpath.join(index_dir, INDEX_JSON): render_index_json(index),
    }


def run(root: Path, write: bool = False) -> list[str]:
    try:
        records_rel = records_directory(root)
    except ValueError as exc:
        return [str(exc)]
    records, errors = load_records(root, records_rel)
    errors.extend(check_authority(records))
    if errors:
        # Do not publish an index derived from invalid records.
        return sorted(set(errors), key=errors.index)
    outputs = generated_outputs(root, records_rel, records)
    for rel, content in outputs.items():
        target = root / rel
        if target.is_symlink():
            errors.append(f"{rel}: must not be a symbolic link")
            continue
        current = target.read_text(encoding="utf-8") if target.is_file() else None
        if current == content:
            continue
        if write:
            with open(target, "w", encoding="utf-8", newline="\n") as handle:
                handle.write(content)
        else:
            state = "missing" if current is None else "stale"
            errors.append(f"{rel}: {state}; run python3 tools/governance/check.py --write")
    return errors


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="regenerate the index before validating")
    parser.add_argument("--root", type=Path, default=None, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    errors = run(args.root or repository_root(), write=args.write)
    if errors:
        print("architecture: FAILED", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("architecture: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
