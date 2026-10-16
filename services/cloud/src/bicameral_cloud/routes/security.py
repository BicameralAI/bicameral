"""Public security-disclosure metadata."""

from __future__ import annotations

from fastapi import APIRouter
from fastapi.responses import PlainTextResponse

router = APIRouter(tags=["security"])

_SECURITY_TXT = """Contact: mailto:devs@bicameral-ai.com
Expires: 2027-06-30T23:59:59Z
Preferred-Languages: en
Canonical: https://app.bicameral-ai.com/.well-known/security.txt
"""


@router.get(
    "/.well-known/security.txt",
    response_class=PlainTextResponse,
    include_in_schema=False,
)
async def security_txt() -> PlainTextResponse:
    """Publish the monitored vulnerability-disclosure contact per RFC 9116."""

    return PlainTextResponse(_SECURITY_TXT, media_type="text/plain")
