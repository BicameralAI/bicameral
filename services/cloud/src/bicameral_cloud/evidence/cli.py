"""Command-line entrypoint for the reconciliation evidence tooling.

Subcommands:

* ``export``   — run every collector and emit a machine-readable evidence bundle.
                 Runs without live credentials, emitting typed
                 ``environment_inconclusive`` items instead of failing when a
                 CLI/credential/infrastructure is absent.
* ``compare``  — compare a produced bundle against the repository's tracked state.
* ``validate`` — validate a bundle document against the bundle schema.
* ``plan``     — print the exact command plan (no execution); the owner runbook.

The tool always exits ``0`` for a successfully produced artifact even when the
result is ``environment_inconclusive`` — an inconclusive environment is a valid,
typed outcome, not a tool failure. Use ``--fail-on-inconclusive`` / ``--fail-on-drift``
to opt into non-zero exits for gating.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from bicameral_cloud.evidence import jsonutil
from bicameral_cloud.evidence.bundle import EvidenceBundle, validate_bundle
from bicameral_cloud.evidence.collect import ALL_COLLECTORS, collect_all
from bicameral_cloud.evidence.comparator import OverallStatus, compare
from bicameral_cloud.evidence.redaction import Redactor, canonical_json
from bicameral_cloud.evidence.runner import SubprocessRunner
from bicameral_cloud.evidence.tracked_state import derive_tracked_state

_REPO_ROOT = Path(__file__).resolve().parents[3]
_MIGRATIONS_DIR = _REPO_ROOT / "supabase" / "migrations"
_EXPECTED_CONFIG = _REPO_ROOT / "docs" / "evidence" / "expected-config.json"

# Environment values that identify the observed environment and must be scrubbed
# from any retained evidence, mapped to a stable redaction label.
_REDACTION_ENV = {
    "GCP_PROJECT": "gcp_project",
    "SUPABASE_PROJECT_REF": "supabase_project_ref",
    "SUPABASE_DB_HOST": "supabase_db_host",
}


def _redactor_from_env(environ: dict[str, str]) -> Redactor:
    identifiers = {
        environ[name]: label
        for name, label in _REDACTION_ENV.items()
        if environ.get(name)
    }
    return Redactor(identifiers=identifiers)


def _print(document: object, out_path: str | None) -> None:
    text = canonical_json(document)
    if out_path:
        Path(out_path).write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)


def _cmd_export(args: argparse.Namespace) -> int:
    environ = dict(os.environ)
    redactor = _redactor_from_env(environ)
    bundle = collect_all(
        SubprocessRunner(environ=environ),
        redactor,
        environment_id=args.environment,
        environment_description=args.description,
    )
    _print(bundle.to_json(), args.output)
    if args.fail_on_inconclusive and bundle.has_inconclusive():
        print("evidence bundle has inconclusive items", file=sys.stderr)
        return 3
    return 0


def _load_bundle(path: Path) -> EvidenceBundle | None:
    document = jsonutil.loads(path.read_text(encoding="utf-8"))
    errors = validate_bundle(document)
    if errors:
        print("bundle failed schema validation:", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return None
    return EvidenceBundle.from_json(document)


def _cmd_compare(args: argparse.Namespace) -> int:
    bundle = _load_bundle(Path(args.bundle))
    if bundle is None:
        return 2
    tracked = derive_tracked_state(_MIGRATIONS_DIR, _EXPECTED_CONFIG)
    report = compare(tracked, bundle)
    _print(report.to_json(), args.output)
    if args.fail_on_drift and report.overall == OverallStatus.DRIFT_DETECTED:
        return 4
    if (
        args.fail_on_inconclusive
        and report.overall == OverallStatus.ENVIRONMENT_INCONCLUSIVE
    ):
        return 3
    return 0


def _cmd_validate(args: argparse.Namespace) -> int:
    document = jsonutil.loads(Path(args.bundle).read_text(encoding="utf-8"))
    errors = validate_bundle(document)
    if errors:
        print("bundle failed schema validation:", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1
    print("bundle OK")
    return 0


def _cmd_plan(args: argparse.Namespace) -> int:
    plan = [
        {
            "id": collector.spec.id,
            "category": collector.spec.category,
            "description": collector.spec.description,
            "command": collector.spec.command,
            "required_env": collector.spec.required_env,
        }
        for collector in ALL_COLLECTORS
    ]
    _print({"commands": plan}, args.output)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="reconciliation-evidence", description=__doc__
    )
    sub = parser.add_subparsers(dest="command", required=True)

    export = sub.add_parser("export", help="run collectors and emit an evidence bundle")
    export.add_argument(
        "--environment",
        default="unspecified",
        help="environment identifier (e.g. staging)",
    )
    export.add_argument(
        "--description", default="", help="human-readable environment description"
    )
    export.add_argument(
        "--output", default=None, help="write bundle to this path (default: stdout)"
    )
    export.add_argument("--fail-on-inconclusive", action="store_true")
    export.set_defaults(func=_cmd_export)

    compare_cmd = sub.add_parser(
        "compare", help="compare a bundle against tracked state"
    )
    compare_cmd.add_argument("bundle", help="path to a produced evidence bundle")
    compare_cmd.add_argument("--output", default=None)
    compare_cmd.add_argument("--fail-on-drift", action="store_true")
    compare_cmd.add_argument("--fail-on-inconclusive", action="store_true")
    compare_cmd.set_defaults(func=_cmd_compare)

    validate = sub.add_parser("validate", help="validate a bundle document")
    validate.add_argument("bundle")
    validate.set_defaults(func=_cmd_validate)

    plan = sub.add_parser("plan", help="print the exact command plan without running")
    plan.add_argument("--output", default=None)
    plan.set_defaults(func=_cmd_plan)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    result: int = args.func(args)
    return result


if __name__ == "__main__":
    raise SystemExit(main())
