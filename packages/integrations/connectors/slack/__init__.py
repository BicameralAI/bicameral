# SPDX-License-Identifier: Apache-2.0
"""Slack connector package."""

from .connector import SlackConnector, parse_message

__all__ = ["SlackConnector", "parse_message"]
