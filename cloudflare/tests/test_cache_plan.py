# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
from odoo.tests.common import BaseCase, tagged

from ..utils import cache_plan

BYPASS = {"action": "set_cache_settings", "action_parameters": {"cache": False},
          "expression": '(http.cookie contains "session_id")', "description": "Bypass", "enabled": True}
CACHE = {"action": "set_cache_settings", "action_parameters": {"cache": True, "edge_ttl": {"mode": "respect_origin"}},
         "expression": '(starts_with(http.request.uri.path, "/relay/apt/"))', "description": "Downloads", "enabled": True}
OTHER = {"id": "x1", "version": "2", "last_updated": "t", "action": "set_cache_settings",
         "action_parameters": {"cache": False}, "expression": "(true)", "description": "Hand made", "enabled": True}


@tagged("post_install", "-at_install")
class TestCachePlanMerge(BaseCase):
    """The pure merge: managed rules in, unmanaged rules carried through."""

    def test_empty_zone_creates_both_bypass_first(self):
        plan = cache_plan.merge([], [CACHE, BYPASS], {"Bypass", "Downloads"})
        self.assertEqual([r["description"] for r in plan["rules"]], ["Bypass", "Downloads"])
        self.assertEqual([e["action"] for e in plan["entries"]], ["create", "create"])
        self.assertFalse(plan["problems"])

    def test_unmanaged_rule_survives_in_place_without_read_only_fields(self):
        plan = cache_plan.merge([dict(OTHER)], [BYPASS, CACHE], {"Bypass", "Downloads"})
        self.assertEqual([r["description"] for r in plan["rules"]], ["Bypass", "Hand made", "Downloads"])
        kept = plan["rules"][1]
        self.assertEqual(kept["id"], "x1")
        self.assertNotIn("version", kept)
        self.assertIn("keep", [e["action"] for e in plan["entries"]])

    def test_existing_managed_rule_is_updated_in_place_by_id_and_unchanged_is_not_a_write(self):
        remote = [dict(BYPASS, id="b1"), dict(CACHE, id="c1", expression="(false)")]
        plan = cache_plan.merge(remote, [BYPASS, CACHE], {"Bypass", "Downloads"})
        by = {e["name"]: e["action"] for e in plan["entries"]}
        self.assertEqual(by, {"Bypass": "unchanged", "Downloads": "update"})
        self.assertEqual({r["description"]: r["id"] for r in plan["rules"]}, {"Bypass": "b1", "Downloads": "c1"})

    def test_inactive_managed_rule_is_the_only_thing_ever_deleted(self):
        remote = [dict(BYPASS, id="b1"), dict(OTHER)]
        plan = cache_plan.merge(remote, [], {"Bypass"})
        self.assertEqual([r["description"] for r in plan["rules"]], ["Hand made"])
        self.assertEqual({e["name"]: e["action"] for e in plan["entries"]}, {"Bypass": "delete", "Hand made": "keep"})

    def test_duplicate_names_are_refused(self):
        plan = cache_plan.merge([dict(BYPASS, id="1"), dict(BYPASS, id="2")], [BYPASS], {"Bypass"})
        self.assertTrue(plan["problems"])
        plan = cache_plan.merge([], [BYPASS, BYPASS], {"Bypass"})
        self.assertTrue(plan["problems"])

    def test_hash_changes_when_the_zone_changes(self):
        a = cache_plan.merge([], [BYPASS], {"Bypass"})["hash"]
        b = cache_plan.merge([dict(OTHER)], [BYPASS], {"Bypass"})["hash"]
        self.assertNotEqual(a, b)
