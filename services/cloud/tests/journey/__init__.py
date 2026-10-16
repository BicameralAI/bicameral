"""Deterministic browser UserJourney harnesses for Bicameral Cloud.

The ``hosted_project_creation`` journey (issue #114) drives a real browser
through the hosted dashboard's Create Project flow against the live Cloud app
and its real registration/discovery HTTP contracts, entirely on loopback with
no production credentials. See
``test_hosted_project_creation_journey.py`` for the executable journey.
"""
