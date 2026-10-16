# SPDX-License-Identifier: Apache-2.0
"""Zendesk connector package."""

from .connector import ZendeskConnector, parse_ticket

__all__ = ["ZendeskConnector", "parse_ticket"]
