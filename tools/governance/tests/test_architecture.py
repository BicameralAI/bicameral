"""Tests for tools/governance/check_architecture.py."""

from __future__ import annotations

import json
import re
import unittest

import check_architecture as arch
from tests.fixtures import REPO_ROOT, TempRepoTestCase, record_path, record_text


def parse(record_id: str, slug: str = "record", **kwargs):
    return arch.parse_record(record_path(record_id, slug), record_text(record_id, **kwargs))


def authority_errors(*texts: tuple[str, str]) -> list[str]:
    records, errors = [], []
    for path, text in texts:
        record, record_errors = arch.parse_record(path, text)
        errors.extend(record_errors)
        if record is not None:
            records.append(record)
    return errors + arch.check_authority(records)


def rec(record_id: str, **kwargs) -> tuple[str, str]:
    return record_path(record_id), record_text(record_id, **kwargs)


class AssertMixin:
    def assertErrorContains(self, errors: list[str], fragment: str) -> None:
        self.assertTrue(
            any(fragment in error for error in errors),
            f"expected an error containing {fragment!r}, got {errors}",
        )


class RecordParsingTests(AssertMixin, unittest.TestCase):
    def test_valid_records_of_each_kind(self) -> None:
        for kind in arch.KINDS:
            with self.subTest(kind=kind):
                record, errors = parse("ARCH-0001", kind=kind)
                self.assertEqual(errors, [])
                self.assertEqual(record.kind, kind)

    def test_list_and_scalar_values(self) -> None:
        record, errors = parse("ARCH-0001", governs=["a-topic", "b-topic"], references=[])
        self.assertEqual(errors, [])
        self.assertEqual(record.governs, ["a-topic", "b-topic"])
        self.assertEqual(record.references, [])

    def test_missing_front_matter(self) -> None:
        _, errors = arch.parse_record(record_path("ARCH-0001"), "# ARCH-0001: x\n")
        self.assertErrorContains(errors, "must start with '---' front matter")

    def test_unterminated_front_matter(self) -> None:
        _, errors = arch.parse_record(record_path("ARCH-0001"), "---\nid: ARCH-0001\n")
        self.assertErrorContains(errors, "not terminated")

    def test_unsupported_front_matter_syntax(self) -> None:
        text = record_text("ARCH-0001", extra_front_matter="  nested: value\n")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "unsupported front matter line")

    def test_malformed_list(self) -> None:
        text = record_text("ARCH-0001").replace("governs: [topic-0001]", "governs: [topic-0001")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "malformed list for 'governs'")

    def test_unknown_key(self) -> None:
        text = record_text("ARCH-0001", extra_front_matter="priority: high\n")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "unsupported front matter keys ['priority']")

    def test_missing_key(self) -> None:
        text = record_text("ARCH-0001").replace("references: []\n", "")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "missing front matter keys ['references']")

    def test_duplicate_key(self) -> None:
        text = record_text("ARCH-0001", extra_front_matter="kind: design\n")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "duplicate key 'kind'")

    def test_derived_fields_cannot_be_authored(self) -> None:
        for key in ("required_by", "superseded_by"):
            with self.subTest(key=key):
                text = record_text("ARCH-0001", extra_front_matter=f"{key}: [ARCH-0002]\n")
                _, errors = arch.parse_record(record_path("ARCH-0001"), text)
                self.assertErrorContains(errors, f"'{key}' is derived")

    def test_scalar_field_given_as_list(self) -> None:
        text = record_text("ARCH-0001").replace("kind: decision", "kind: [decision]")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "kind must be a scalar value")

    def test_list_field_given_as_scalar(self) -> None:
        text = record_text("ARCH-0001").replace("depends_on: []", "depends_on: ARCH-0002")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "depends_on must be a list")

    def test_invalid_id(self) -> None:
        text = record_text("ARCH-0001").replace("id: ARCH-0001", "id: ARCH-1")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "must match ARCH-NNNN")

    def test_filename_must_match_id(self) -> None:
        _, errors = arch.parse_record(record_path("ARCH-0009"), record_text("ARCH-0001"))
        self.assertErrorContains(errors, "filename id ARCH-0009 does not match id ARCH-0001")

    def test_filename_slug_format(self) -> None:
        path = "docs/architecture/records/ARCH-0001-Bad_Slug.md"
        _, errors = arch.parse_record(path, record_text("ARCH-0001"))
        self.assertErrorContains(errors, "filename must match ARCH-NNNN-slug.md")

    def test_invalid_kind_and_status(self) -> None:
        _, errors = parse("ARCH-0001", kind="memo", status="draft")
        self.assertErrorContains(errors, "kind 'memo' must be one of")
        self.assertErrorContains(errors, "status 'draft' must be one of")

    def test_invalid_dates(self) -> None:
        _, errors = parse("ARCH-0001", date="2026-02-30")
        self.assertErrorContains(errors, "not a valid calendar date")
        _, errors = parse("ARCH-0001", date="20261016")
        self.assertErrorContains(errors, "must be YYYY-MM-DD")

    def test_invalid_owner(self) -> None:
        _, errors = parse("ARCH-0001", owners=["jin"])
        self.assertErrorContains(errors, "invalid owner handle 'jin'")

    def test_empty_owners(self) -> None:
        _, errors = parse("ARCH-0001", owners=[])
        self.assertErrorContains(errors, "owners must not be empty")

    def test_invalid_topic(self) -> None:
        _, errors = parse("ARCH-0001", governs=["Repository Layout"])
        self.assertErrorContains(errors, "must be lowercase-kebab-case")

    def test_relationship_entries_must_be_ids(self) -> None:
        _, errors = parse("ARCH-0001", depends_on=["architecture-records"])
        self.assertErrorContains(errors, "depends_on entry 'architecture-records' must be an ARCH-NNNN id")

    def test_duplicate_relationship_entry(self) -> None:
        _, errors = parse("ARCH-0002", depends_on=["ARCH-0001", "ARCH-0001"])
        self.assertErrorContains(errors, "duplicate depends_on entries ['ARCH-0001']")

    def test_duplicate_topic_entry(self) -> None:
        _, errors = parse("ARCH-0001", governs=["a", "a"])
        self.assertErrorContains(errors, "duplicate governs entries ['a']")

    def test_duplicate_cross_list_relationship(self) -> None:
        _, errors = parse("ARCH-0002", depends_on=["ARCH-0001"], references=["ARCH-0001"])
        self.assertErrorContains(errors, "ARCH-0001 appears in both depends_on and references")

    def test_first_heading_must_match(self) -> None:
        text = record_text("ARCH-0001", title="Right").replace("# ARCH-0001: Right", "# Wrong")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "first heading must be '# ARCH-0001: Right'")

    def test_required_sections_per_kind(self) -> None:
        cases = {
            "decision": ("## Consequences", "Consequences"),
            "design": ("## Design", "Design"),
            "contract": ("## Scope", "Scope"),
        }
        for kind, (heading, name) in cases.items():
            with self.subTest(kind=kind):
                text = record_text("ARCH-0001", kind=kind).replace(heading, "## Other")
                _, errors = arch.parse_record(record_path("ARCH-0001"), text)
                self.assertErrorContains(errors, f"{kind} records require a '## {name}' section")

    def test_undeclared_body_reference(self) -> None:
        body = "## Context\n\nSee ARCH-0007.\n\n## Decision\n\nd\n\n## Consequences\n\nq\n"
        _, errors = parse("ARCH-0001", body=body)
        self.assertErrorContains(errors, "body references ['ARCH-0007'] without declaring them")

    def test_body_reference_declared_in_references_is_allowed(self) -> None:
        body = "## Context\n\nSee ARCH-0007.\n\n## Decision\n\nd\n\n## Consequences\n\nq\n"
        _, errors = parse("ARCH-0001", body=body, references=["ARCH-0007"])
        self.assertEqual(errors, [])

    def test_body_self_reference_is_allowed(self) -> None:
        body = "## Context\n\nARCH-0001 is this.\n\n## Decision\n\nd\n\n## Consequences\n\nq\n"
        _, errors = parse("ARCH-0001", body=body)
        self.assertEqual(errors, [])

    def test_crlf_rejected(self) -> None:
        text = record_text("ARCH-0001").replace("\n", "\r\n")
        _, errors = arch.parse_record(record_path("ARCH-0001"), text)
        self.assertErrorContains(errors, "LF line endings")


class AuthorityTests(AssertMixin, unittest.TestCase):
    def test_valid_accepted_chain(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["a"]),
            rec("ARCH-0002", status="accepted", governs=["b"], depends_on=["ARCH-0001"]),
        )
        self.assertEqual(errors, [])

    def test_accepted_must_govern_a_topic(self) -> None:
        errors = authority_errors(rec("ARCH-0001", status="accepted", governs=[]))
        self.assertErrorContains(errors, "accepted records must govern at least one topic")

    def test_proposed_may_govern_nothing(self) -> None:
        self.assertEqual(authority_errors(rec("ARCH-0001", governs=[])), [])

    def test_duplicate_accepted_topic(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["layout"]),
            rec("ARCH-0002", status="accepted", governs=["layout"]),
        )
        self.assertErrorContains(
            errors, "topic 'layout' is governed by multiple accepted records ['ARCH-0001', 'ARCH-0002']"
        )

    def test_proposed_may_claim_an_accepted_topic(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["layout"]),
            rec("ARCH-0002", status="proposed", governs=["layout"]),
        )
        self.assertEqual(errors, [])

    def test_historical_topic_does_not_conflict(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["layout"]),
            rec("ARCH-0002", status="rejected", governs=["layout"]),
            rec("ARCH-0003", status="deprecated", governs=["layout"]),
        )
        self.assertEqual(errors, [])

    def test_unknown_dependency(self) -> None:
        errors = authority_errors(rec("ARCH-0002", depends_on=["ARCH-0099"]))
        self.assertErrorContains(errors, "depends_on references unknown record ARCH-0099")

    def test_unknown_reference_and_supersedes(self) -> None:
        errors = authority_errors(rec("ARCH-0002", references=["ARCH-0098"], supersedes=["ARCH-0099"]))
        self.assertErrorContains(errors, "references references unknown record ARCH-0098")
        self.assertErrorContains(errors, "supersedes references unknown record ARCH-0099")

    def test_self_reference(self) -> None:
        errors = authority_errors(rec("ARCH-0001", depends_on=["ARCH-0001"]))
        self.assertErrorContains(errors, "depends_on must not reference the record itself")

    def test_duplicate_ids(self) -> None:
        errors = authority_errors(
            (record_path("ARCH-0001", "one"), record_text("ARCH-0001")),
            (record_path("ARCH-0001", "two"), record_text("ARCH-0001")),
        )
        self.assertErrorContains(errors, "duplicate id ARCH-0001")

    def test_dependency_cycle(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", depends_on=["ARCH-0003"]),
            rec("ARCH-0002", depends_on=["ARCH-0001"]),
            rec("ARCH-0003", depends_on=["ARCH-0002"]),
        )
        self.assertErrorContains(errors, "depends_on cycle: ARCH-0001 -> ARCH-0003 -> ARCH-0002 -> ARCH-0001")

    def test_supersession_cycle(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="superseded", supersedes=["ARCH-0002"]),
            rec("ARCH-0002", status="superseded", supersedes=["ARCH-0001"]),
        )
        self.assertErrorContains(errors, "supersedes cycle")

    def test_accepted_dependency_on_proposed(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="proposed"),
            rec("ARCH-0002", status="accepted", depends_on=["ARCH-0001"]),
        )
        self.assertErrorContains(
            errors, "accepted record depends on ARCH-0001, which is proposed"
        )

    def test_accepted_dependency_on_historical(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="deprecated"),
            rec("ARCH-0002", status="accepted", depends_on=["ARCH-0001"]),
        )
        self.assertErrorContains(errors, "which is deprecated")

    def test_proposed_dependency_on_accepted_or_proposed(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted"),
            rec("ARCH-0002", status="proposed"),
            rec("ARCH-0003", status="proposed", depends_on=["ARCH-0001", "ARCH-0002"]),
        )
        self.assertEqual(errors, [])

    def test_proposed_dependency_on_historical(self) -> None:
        for status in arch.HISTORICAL:
            with self.subTest(status=status):
                records = [rec("ARCH-0001", status=status)]
                if status == "superseded":
                    records.append(rec("ARCH-0003", status="accepted", supersedes=["ARCH-0001"]))
                records.append(rec("ARCH-0002", status="proposed", depends_on=["ARCH-0001"]))
                errors = authority_errors(*records)
                self.assertErrorContains(
                    errors, f"proposed record depends on ARCH-0001, which is {status}"
                )

    def test_proposed_replacement_of_accepted_is_valid_intermediate_state(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["layout"]),
            rec("ARCH-0002", status="proposed", governs=["layout"], supersedes=["ARCH-0001"]),
        )
        self.assertEqual(errors, [])

    def test_effective_supersession(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="superseded", governs=["layout"]),
            rec("ARCH-0002", status="accepted", governs=["layout"], supersedes=["ARCH-0001"]),
        )
        self.assertEqual(errors, [])

    def test_superseded_and_deprecated_replacements_keep_history_valid(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="superseded"),
            rec("ARCH-0002", status="superseded", supersedes=["ARCH-0001"]),
            rec("ARCH-0003", status="deprecated", supersedes=["ARCH-0002"]),
        )
        self.assertEqual(errors, [])

    def test_invalid_supersession_accepted_replaces_still_accepted(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["a"]),
            rec("ARCH-0002", status="accepted", governs=["b"], supersedes=["ARCH-0001"]),
        )
        self.assertErrorContains(
            errors, "accepted record supersedes ARCH-0001, which is accepted; mark ARCH-0001 superseded"
        )

    def test_invalid_supersession_proposed_replaces_non_accepted(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="proposed"),
            rec("ARCH-0002", status="proposed", supersedes=["ARCH-0001"]),
        )
        self.assertErrorContains(
            errors, "a proposed replacement may only supersede an accepted record; ARCH-0001 is proposed"
        )

    def test_superseded_without_replacement(self) -> None:
        errors = authority_errors(rec("ARCH-0001", status="superseded"))
        self.assertErrorContains(errors, "superseded records must be superseded by an accepted")

    def test_superseded_by_only_a_proposal_is_invalid(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="superseded"),
            rec("ARCH-0002", status="proposed", supersedes=["ARCH-0001"]),
        )
        self.assertErrorContains(errors, "superseded records must be superseded by an accepted")

    def test_superseded_by_rejected_replacement_is_invalid(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="superseded"),
            rec("ARCH-0002", status="rejected", supersedes=["ARCH-0001"]),
        )
        self.assertErrorContains(errors, "superseded records must be superseded by an accepted")

    def test_rejected_replacement_leaves_predecessor_accepted(self) -> None:
        errors = authority_errors(
            rec("ARCH-0001", status="accepted", governs=["layout"]),
            rec("ARCH-0002", status="rejected", governs=["layout"], supersedes=["ARCH-0001"]),
        )
        self.assertEqual(errors, [])


class RealRecordTests(AssertMixin, unittest.TestCase):
    """Exercise the authority rules against the repository's own records (read-only)."""

    def load(self, record_id: str) -> tuple[str, str]:
        directory = REPO_ROOT / "docs" / "architecture" / "records"
        matches = sorted(directory.glob(f"{record_id}-*.md"))
        self.assertEqual(len(matches), 1, f"expected exactly one {record_id} record")
        path = matches[0]
        return f"docs/architecture/records/{path.name}", path.read_text(encoding="utf-8")

    @staticmethod
    def with_status(item: tuple[str, str], status: str) -> tuple[str, str]:
        path, text = item
        return path, re.sub(r"(?m)^status: \w+$", f"status: {status}", text, count=1)

    def test_foundational_records_are_valid_as_committed(self) -> None:
        self.assertEqual(authority_errors(self.load("ARCH-0001"), self.load("ARCH-0002")), [])

    def test_arch_0002_depends_on_arch_0001(self) -> None:
        record, errors = arch.parse_record(*self.load("ARCH-0002"))
        self.assertEqual(errors, [])
        self.assertIn("ARCH-0001", record.depends_on)

    def test_accepting_arch_0002_while_arch_0001_is_proposed_fails(self) -> None:
        errors = authority_errors(
            self.with_status(self.load("ARCH-0001"), "proposed"),
            self.with_status(self.load("ARCH-0002"), "accepted"),
        )
        self.assertErrorContains(errors, "accepted record depends on ARCH-0001, which is proposed")

    def test_accepting_both_is_valid(self) -> None:
        errors = authority_errors(
            self.with_status(self.load("ARCH-0001"), "accepted"),
            self.with_status(self.load("ARCH-0002"), "accepted"),
        )
        self.assertEqual(errors, [])


class IndexTests(AssertMixin, TempRepoTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.write(
            "governance/structure.json",
            json.dumps({"canonical": {"architecture_records": "docs/architecture/records"}}),
        )
        self.write("docs/architecture/records/README.md", "records\n")

    def add(self, record_id: str, slug: str = "record", **kwargs) -> None:
        self.write(record_path(record_id, slug), record_text(record_id, **kwargs))

    def test_missing_index_fails_and_write_creates_it(self) -> None:
        self.add("ARCH-0001")
        self.assertErrorContains(arch.run(self.root), "docs/architecture/INDEX.md: missing")
        self.assertEqual(arch.run(self.root, write=True), [])
        self.assertEqual(arch.run(self.root), [])

    def test_stale_markdown_index(self) -> None:
        self.add("ARCH-0001")
        arch.run(self.root, write=True)
        self.write("docs/architecture/INDEX.md", self.read("docs/architecture/INDEX.md") + "edit\n")
        self.assertErrorContains(arch.run(self.root), "docs/architecture/INDEX.md: stale")

    def test_stale_json_index_after_record_change(self) -> None:
        self.add("ARCH-0001")
        arch.run(self.root, write=True)
        self.add("ARCH-0001", title="Renamed")
        errors = arch.run(self.root)
        self.assertErrorContains(errors, "docs/architecture/index.json: stale")
        self.assertErrorContains(errors, "docs/architecture/INDEX.md: stale")

    def test_normal_mode_does_not_modify_files(self) -> None:
        self.add("ARCH-0001")
        before = self.snapshot()
        arch.run(self.root)
        self.assertEqual(before, self.snapshot())

    def test_invalid_records_do_not_produce_an_index(self) -> None:
        self.add("ARCH-0001", status="accepted", governs=[])
        errors = arch.run(self.root, write=True)
        self.assertErrorContains(errors, "accepted records must govern at least one topic")
        self.assertFalse((self.root / "docs/architecture/index.json").exists())

    def test_generation_is_deterministic_and_machine_independent(self) -> None:
        self.add("ARCH-0002", depends_on=["ARCH-0001"], governs=["b"],
                 body="## Context\n\nARCH-0001.\n\n## Decision\n\nd\n\n## Consequences\n\nq\n")
        self.add("ARCH-0001", status="accepted", governs=["a"])
        arch.run(self.root, write=True)
        first = (self.read("docs/architecture/INDEX.md"), self.read("docs/architecture/index.json"))
        arch.run(self.root, write=True)
        second = (self.read("docs/architecture/INDEX.md"), self.read("docs/architecture/index.json"))
        self.assertEqual(first, second)
        for text in first:
            self.assertNotIn(str(self.root), text)
            self.assertNotRegex(text, r"\d{2}:\d{2}:\d{2}")

    def test_index_contents(self) -> None:
        self.add("ARCH-0001", status="superseded", governs=["layout"])
        self.add("ARCH-0002", status="accepted", governs=["layout", "owners"],
                 supersedes=["ARCH-0001"])
        self.add("ARCH-0003", status="proposed", governs=["layout"], supersedes=["ARCH-0002"],
                 depends_on=["ARCH-0004"])
        self.add("ARCH-0004", status="accepted", governs=["records"])
        self.add("ARCH-0005", status="rejected", governs=["layout"])
        self.assertEqual(arch.run(self.root, write=True), [])
        index = json.loads(self.read("docs/architecture/index.json"))
        self.assertEqual(
            index["authority"], {"layout": "ARCH-0002", "owners": "ARCH-0002", "records": "ARCH-0004"}
        )
        self.assertEqual(index["proposed_claims"], {"layout": ["ARCH-0003"]})
        self.assertEqual(index["accepted"], ["ARCH-0002", "ARCH-0004"])
        self.assertEqual(index["proposed"], ["ARCH-0003"])
        self.assertEqual(index["historical"], ["ARCH-0001", "ARCH-0005"])
        by_id = {entry["id"]: entry for entry in index["records"]}
        self.assertEqual(by_id["ARCH-0001"]["superseded_by"], ["ARCH-0002"])
        self.assertEqual(by_id["ARCH-0002"]["superseded_by"], [])
        self.assertEqual(by_id["ARCH-0002"]["proposed_replacements"], ["ARCH-0003"])
        self.assertEqual(by_id["ARCH-0004"]["required_by"], ["ARCH-0003"])
        markdown = self.read("docs/architecture/INDEX.md")
        self.assertIn("| `layout` | [ARCH-0002](records/ARCH-0002-record.md) |", markdown)

    def test_proposed_records_never_appear_as_authority(self) -> None:
        self.add("ARCH-0001", status="proposed", governs=["a"])
        self.add("ARCH-0002", status="deprecated", governs=["b"])
        self.assertEqual(arch.run(self.root, write=True), [])
        index = json.loads(self.read("docs/architecture/index.json"))
        self.assertEqual(index["authority"], {})
        self.assertIn("_No topic currently has an accepted authoritative record._",
                      self.read("docs/architecture/INDEX.md"))

    def test_unexpected_file_in_records_directory(self) -> None:
        self.write("docs/architecture/records/notes.md", "notes\n")
        self.assertErrorContains(arch.run(self.root), "only README.md and ARCH-NNNN-slug.md files belong here")


if __name__ == "__main__":
    unittest.main()
