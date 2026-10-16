"""Google Cloud Run / IAM / load-balancer / IAP evidence collectors.

Each collector pairs the exact ``gcloud`` command an authorized owner runs with a
pure parser over its JSON output. Parsers are redaction-safe and never require live
credentials to *parse* fixture output; the command only needs credentials to *run*.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass

from bicameral_cloud.evidence import jsonutil
from bicameral_cloud.evidence.bundle import CommandSpec
from bicameral_cloud.evidence.outcomes import CollectionResult
from bicameral_cloud.evidence.redaction import Redactor

Parser = Callable[[str, Redactor], CollectionResult]


@dataclass(frozen=True)
class Collector:
    spec: CommandSpec
    parse: Parser


_INGRESS_ANNOTATION = "run.googleapis.com/ingress"
_SERVICE_DESCRIBE = [
    "gcloud",
    "run",
    "services",
    "describe",
    "${CLOUD_RUN_SERVICE}",
    "--project",
    "${GCP_PROJECT}",
    "--region",
    "${GCP_REGION}",
    "--format",
    "json",
]


def _parse_service(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.as_object(jsonutil.loads(text), context="cloud run service")
    metadata = jsonutil.get_object(root, "metadata")
    annotations = jsonutil.get_object(metadata, "annotations")
    spec = jsonutil.get_object(root, "spec")
    template = jsonutil.get_object(spec, "template")
    template_spec = jsonutil.get_object(template, "spec")
    status = jsonutil.get_object(root, "status")
    payload: dict[str, object] = {
        "name": jsonutil.get_str(metadata, "name"),
        "ingress": jsonutil.get_str(annotations, _INGRESS_ANNOTATION),
        "service_account": jsonutil.get_str(template_spec, "serviceAccountName"),
        "latest_ready_revision": jsonutil.get_str(status, "latestReadyRevisionName"),
        "latest_created_revision": jsonutil.get_str(
            status, "latestCreatedRevisionName"
        ),
    }
    return CollectionResult.collected(redactor.scrub_payload(payload))


def _parse_ingress(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.as_object(jsonutil.loads(text), context="cloud run service")
    annotations = jsonutil.get_object(
        jsonutil.get_object(root, "metadata"), "annotations"
    )
    ingress = jsonutil.get_str(annotations, _INGRESS_ANNOTATION)
    payload: dict[str, object] = {
        "ingress": ingress or "internal-and-cloud-load-balancing-unset"
    }
    return CollectionResult.collected(redactor.scrub_payload(payload))


def _parse_revision(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.as_object(jsonutil.loads(text), context="cloud run service")
    status = jsonutil.get_object(root, "status")
    traffic_out: list[object] = []
    for entry in jsonutil.get_array(status, "traffic"):
        if isinstance(entry, dict):
            traffic_out.append(
                {
                    "revisionName": jsonutil.get_str(entry, "revisionName"),
                    "percent": entry.get("percent"),
                    "latestRevision": jsonutil.get_bool(entry, "latestRevision"),
                }
            )
    payload: dict[str, object] = {
        "latest_ready_revision": jsonutil.get_str(status, "latestReadyRevisionName"),
        "traffic": traffic_out,
    }
    return CollectionResult.collected(redactor.scrub_payload(payload))


def _parse_service_account(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.as_object(jsonutil.loads(text), context="cloud run service")
    template_spec = jsonutil.get_object(
        jsonutil.get_object(jsonutil.get_object(root, "spec"), "template"), "spec"
    )
    payload: dict[str, object] = {
        "service_account": jsonutil.get_str(template_spec, "serviceAccountName")
    }
    return CollectionResult.collected(redactor.scrub_payload(payload))


def _parse_iam_policy(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.as_object(jsonutil.loads(text), context="cloud run iam policy")
    bindings_out: list[object] = []
    all_users_public = False
    for binding in jsonutil.get_array(root, "bindings"):
        if not isinstance(binding, dict):
            continue
        role = jsonutil.get_str(binding, "role")
        members = [
            m for m in jsonutil.get_array(binding, "members") if isinstance(m, str)
        ]
        if role == "roles/run.invoker" and (
            "allUsers" in members or "allAuthenticatedUsers" in members
        ):
            all_users_public = True
        bindings_out.append({"role": role, "members": sorted(members)})
    payload: dict[str, object] = {
        "bindings": bindings_out,
        "public_invoker": all_users_public,
    }
    return CollectionResult.collected(redactor.scrub_payload(payload))


def _parse_load_balancer(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.loads(text)
    entries = jsonutil.as_array(root, context="backend services")
    if not entries:
        return CollectionResult.absent("no external backend services configured")
    services_out: list[object] = []
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        services_out.append(
            {
                "name": jsonutil.get_str(entry, "name"),
                "loadBalancingScheme": jsonutil.get_str(entry, "loadBalancingScheme"),
                "protocol": jsonutil.get_str(entry, "protocol"),
                "iap_enabled": jsonutil.get_bool(
                    jsonutil.get_object(entry, "iap"), "enabled"
                ),
            }
        )
    return CollectionResult.collected(
        redactor.scrub_payload({"backend_services": services_out})
    )


def _parse_iap(text: str, redactor: Redactor) -> CollectionResult:
    root = jsonutil.as_object(jsonutil.loads(text), context="iap iam policy")
    bindings = jsonutil.get_array(root, "bindings")
    if not bindings:
        return CollectionResult.absent("no IAP web IAM policy present")
    bindings_out: list[object] = []
    for binding in bindings:
        if not isinstance(binding, dict):
            continue
        bindings_out.append(
            {
                "role": jsonutil.get_str(binding, "role"),
                "members": sorted(
                    m
                    for m in jsonutil.get_array(binding, "members")
                    if isinstance(m, str)
                ),
            }
        )
    return CollectionResult.collected(
        redactor.scrub_payload({"bindings": bindings_out})
    )


COLLECTORS: tuple[Collector, ...] = (
    Collector(
        CommandSpec(
            id="gcp.cloud_run.service",
            category="cloud_run_service",
            description=(
                "Cloud Run service description "
                "(name, ingress, service account, revision)."
            ),
            command=_SERVICE_DESCRIBE,
            required_env=["CLOUD_RUN_SERVICE", "GCP_PROJECT", "GCP_REGION"],
        ),
        _parse_service,
    ),
    Collector(
        CommandSpec(
            id="gcp.cloud_run.ingress",
            category="cloud_run_ingress",
            description="Cloud Run ingress restriction annotation.",
            command=_SERVICE_DESCRIBE,
            required_env=["CLOUD_RUN_SERVICE", "GCP_PROJECT", "GCP_REGION"],
        ),
        _parse_ingress,
    ),
    Collector(
        CommandSpec(
            id="gcp.cloud_run.revision",
            category="cloud_run_revision",
            description="Cloud Run ready revision and traffic split.",
            command=_SERVICE_DESCRIBE,
            required_env=["CLOUD_RUN_SERVICE", "GCP_PROJECT", "GCP_REGION"],
        ),
        _parse_revision,
    ),
    Collector(
        CommandSpec(
            id="gcp.cloud_run.service_account",
            category="cloud_run_service_account",
            description="Runtime service account bound to the Cloud Run service.",
            command=_SERVICE_DESCRIBE,
            required_env=["CLOUD_RUN_SERVICE", "GCP_PROJECT", "GCP_REGION"],
        ),
        _parse_service_account,
    ),
    Collector(
        CommandSpec(
            id="gcp.cloud_run.iam_policy",
            category="cloud_run_iam_policy",
            description="Cloud Run invoker IAM policy (detects public exposure).",
            command=[
                "gcloud",
                "run",
                "services",
                "get-iam-policy",
                "${CLOUD_RUN_SERVICE}",
                "--project",
                "${GCP_PROJECT}",
                "--region",
                "${GCP_REGION}",
                "--format",
                "json",
            ],
            required_env=["CLOUD_RUN_SERVICE", "GCP_PROJECT", "GCP_REGION"],
        ),
        _parse_iam_policy,
    ),
    Collector(
        CommandSpec(
            id="gcp.load_balancer.backend_services",
            category="load_balancer",
            description=(
                "External HTTPS load-balancer backend services fronting "
                "Cloud Run (when present)."
            ),
            command=[
                "gcloud",
                "compute",
                "backend-services",
                "list",
                "--project",
                "${GCP_PROJECT}",
                "--format",
                "json",
            ],
            required_env=["GCP_PROJECT"],
        ),
        _parse_load_balancer,
    ),
    Collector(
        CommandSpec(
            id="gcp.iap.web_iam_policy",
            category="iap",
            description="Identity-Aware Proxy web IAM policy (when present).",
            command=[
                "gcloud",
                "iap",
                "web",
                "get-iam-policy",
                "--project",
                "${GCP_PROJECT}",
                "--resource-type",
                "backend-services",
                "--format",
                "json",
            ],
            required_env=["GCP_PROJECT"],
        ),
        _parse_iap,
    ),
)
