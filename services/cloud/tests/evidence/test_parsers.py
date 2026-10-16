"""Parser tests for the GCP and Supabase collectors."""

from __future__ import annotations

from bicameral_cloud.evidence import gcp, supabase
from bicameral_cloud.evidence.outcomes import CollectionStatus
from bicameral_cloud.evidence.redaction import Redactor
from tests.evidence.helpers import load_fixture

R = Redactor()


def _parser(collectors: tuple[gcp.Collector, ...], collector_id: str) -> gcp.Parser:
    for collector in collectors:
        if collector.spec.id == collector_id:
            return collector.parse
    raise KeyError(collector_id)


def test_public_iam_policy_flags_public_invoker() -> None:
    parse = _parser(gcp.COLLECTORS, "gcp.cloud_run.iam_policy")
    result = parse(load_fixture("gcp/iam_policy.public.json"), R)
    assert result.status == CollectionStatus.COLLECTED
    assert result.payload is not None
    assert result.payload["public_invoker"] is True


def test_private_iam_policy_is_not_public() -> None:
    parse = _parser(gcp.COLLECTORS, "gcp.cloud_run.iam_policy")
    result = parse(load_fixture("gcp/iam_policy.private.json"), R)
    assert result.payload is not None
    assert result.payload["public_invoker"] is False


def test_ingress_is_extracted() -> None:
    parse = _parser(gcp.COLLECTORS, "gcp.cloud_run.ingress")
    result = parse(load_fixture("gcp/service.hardened.json"), R)
    assert result.payload is not None
    assert result.payload["ingress"] == "internal-and-cloud-load-balancing"


def test_empty_load_balancer_is_absent() -> None:
    parse = _parser(gcp.COLLECTORS, "gcp.load_balancer.backend_services")
    result = parse(load_fixture("gcp/backend_services.empty.json"), R)
    assert result.status == CollectionStatus.ABSENT


def test_present_load_balancer_is_collected() -> None:
    parse = _parser(gcp.COLLECTORS, "gcp.load_balancer.backend_services")
    result = parse(load_fixture("gcp/backend_services.present.json"), R)
    assert result.status == CollectionStatus.COLLECTED


def test_iap_absent_when_no_bindings() -> None:
    parse = _parser(gcp.COLLECTORS, "gcp.iap.web_iam_policy")
    result = parse(load_fixture("gcp/iap.absent.json"), R)
    assert result.status == CollectionStatus.ABSENT


def test_tables_are_sorted() -> None:
    parse = _parser(supabase.COLLECTORS, "supabase.tables")
    result = parse(load_fixture("supabase/tables.baseline.json"), R)
    assert result.payload is not None
    assert result.payload["tables"] == ["design_partner_allowlist", "product_directory"]


def test_runtime_principal_groups_by_principal() -> None:
    parse = _parser(supabase.COLLECTORS, "supabase.runtime_principal")
    result = parse(load_fixture("supabase/runtime_principal.hardened.json"), R)
    assert result.payload is not None
    principals = result.payload["principals"]
    assert isinstance(principals, dict)
    assert set(principals) == {"anon", "authenticated", "service_role"}


def test_empty_psql_output_parses_as_no_rows() -> None:
    parse = _parser(supabase.COLLECTORS, "supabase.policies")
    result = parse("", R)
    assert result.status == CollectionStatus.COLLECTED
    assert result.payload is not None
    assert result.payload["policies"] == []
