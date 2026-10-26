# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.exceptions import AccessError
from odoo.tests.common import tagged

from . import common
from . import test_persona_support as personas_module
from .test_persona_support import CATALOGUE, PersonaError, build_persona, personas


@tagged("post_install", "-at_install")
class TestPersonas(common.HamsTransactionCase):
    """The persona catalogue, the choice of personas from what a test targets, and the runner."""

    # Tests [@ANCHOR: zero_sudo:persona_build]
    # Tests [@ANCHOR: zero_sudo:persona_object]
    def test_catalogue_users_have_the_shape_production_users_have(self):
        shapes = {"public": (False, False), "portal": (True, False), "internal": (False, False)}
        for key, spec in CATALOGUE.items():
            needed = [self.env.ref(x, raise_if_not_found=False) for x in spec.get("groups", ())]
            if not all(needed):
                with self.assertRaises(PersonaError, msg=f"{key} needs a module that is not installed"):
                    build_persona(self.env, key)
                continue
            persona = build_persona(self.env, key)
            user = persona.user
            self.assertEqual(bool(user.share), shapes[spec["kind"]][0] or spec["kind"] == "public", key)
            if spec.get("website") == "none":
                self.assertFalse(user.website_id, f"{key}: no production user has a website")
            if spec.get("website") == "own":
                self.assertTrue(user.website_id, key)
            if spec["kind"] != "public" and "website_slug" in user._fields:
                self.assertTrue(user.website_slug, f"{key}: production users have a member site")
            for xmlid in spec.get("groups", ()):
                self.assertIn(self.env.ref(xmlid), user.all_group_ids, key)

    # Tests [@ANCHOR: zero_sudo:persona_build]
    def test_an_unknown_persona_fails(self):
        with self.assertRaises(PersonaError):
            build_persona(self.env, "nobody_in_particular")

    # Tests [@ANCHOR: zero_sudo:persona_decorator]
    def test_a_test_that_names_no_persona_fails(self):
        with self.assertRaises(PersonaError):
            personas()

    # Tests [@ANCHOR: zero_sudo:persona_decorator]
    # Tests [@ANCHOR: zero_sudo:persona_savepoint_runner]
    def test_the_body_runs_once_per_persona_and_each_run_is_rolled_back(self):
        seen = []

        @personas("public", "portal_member", "admin")
        def body(case, persona):
            seen.append((persona.key, case.env.cr.rowcount is not None))
            case.env["res.partner"].create({"name": f"Rolled back for {persona.key}"})

        body(self)
        self.assertEqual([key for key, _ in seen], ["public", "portal_member", "admin"])
        left = self.env["res.partner"].search_count([("name", "=like", "Rolled back for %")])
        self.assertEqual(left, 0)

    # Tests [@ANCHOR: zero_sudo:persona_model_derivation]
    # Tests [@ANCHOR: zero_sudo:persona_assert_access]
    @personas(model="ir.cron", mode="write")
    def test_a_model_persona_set_names_the_group_and_a_user_who_must_be_refused(self, persona):
        crons = self.env["ir.cron"].with_context(active_test=False).search([], limit=1)
        self.assert_persona_access(persona, crons, "write")
        if persona.key == "admin":
            self.assertTrue(persona.allowed)
        if persona.key in ("public", "portal_member", "internal_staff"):
            self.assertFalse(persona.allowed)

    # Tests [@ANCHOR: zero_sudo:persona_model_derivation]
    def test_a_model_with_no_acl_row_derives_nothing(self):
        bare = self.env["ir.model"].search([("access_ids", "=", False), ("transient", "=", False)], limit=1)
        self.assertTrue(bare, "every model has an ACL row; the check cannot be made")
        with self.assertRaises(PersonaError):
            personas_module.model_personas(self.env, bare.model, mode="unlink")

    # Tests [@ANCHOR: zero_sudo:persona_route_derivation]
    def test_route_personas_follow_auth(self):
        by_auth = {}
        for auth in ("none", "public", "user"):
            by_auth[auth] = {p.key: p.allowed for p in personas_module.route_personas(self.env, auth)}
        self.assertEqual(by_auth["none"], {"public": True})
        self.assertTrue(all(by_auth["public"].values()))
        self.assertIn("public", by_auth["public"])
        self.assertFalse(by_auth["user"]["public"])
        self.assertTrue(by_auth["user"]["portal_member"])
        with self.assertRaises(PersonaError):
            personas_module.route_personas(self.env, "password")

    # Tests [@ANCHOR: zero_sudo:persona_route_derivation]
    def test_a_route_that_checks_a_group_refuses_the_users_without_it(self):
        found = {p.key: p.allowed for p in personas_module.route_personas(self.env, "user", ("base.group_system",))}
        self.assertEqual(found, {"public": False, "portal_member": False, "internal_staff": False, "admin": True})

    def _cron_on_currency(self, persona):
        """A cron like the one that failed on hams.com: a code action on a model its user may read, not write."""
        action = self.env["ir.actions.server"].create(
            {
                "name": f"Persona probe {persona.key}",
                "model_id": self.env.ref("base.model_res_currency").id,
                "state": "code",
                "code": "pass",
            }
        )
        return self.env["ir.cron"].create(
            {
                "name": f"Persona probe cron {persona.key}",
                "ir_actions_server_id": action.id,
                "user_id": persona.user.id,
                "interval_number": 1,
                "interval_type": "days",
            }
        )

    # Tests [@ANCHOR: zero_sudo:persona_cron_refusal]
    # Tests [@ANCHOR: zero_sudo:persona_run_cron_action]
    # Tests [@ANCHOR: zero_sudo:persona_cron]
    def test_a_cron_whose_user_cannot_write_its_model_is_refused_as_odoo_refuses_it(self):
        staff = build_persona(self.env, "internal_staff")
        job = self._cron_on_currency(staff)
        self.assertIn("access rights", personas_module.cron_refusal(job))
        with self.assertRaises(AccessError):
            personas_module.run_cron_action(self.env, job)
        # The model method called as that user is not refused, which is how the real failure passed its test.
        self.assertTrue(self.env["res.currency"].with_user(staff.user).search([], limit=1))
        self.assertEqual(personas_module.cron(self.env, job).user, staff.user)
        admin = build_persona(self.env, "admin")
        allowed_job = self._cron_on_currency(admin)
        self.assertIsNone(personas_module.cron_refusal(allowed_job))
        personas_module.run_cron_action(self.env, allowed_job)

    # Tests [@ANCHOR: zero_sudo:persona_service]
    def test_a_service_account_is_the_installed_record(self):
        persona = personas_module.service(self.env, "base.user_root")
        self.assertEqual(persona.user.id, 1)
        with self.assertRaises(PersonaError):
            personas_module.service(self.env, "base.group_user")

    # Tests [@ANCHOR: zero_sudo:persona_rule_attributes]
    # Tests [@ANCHOR: zero_sudo:persona_rule_empty_attribute]
    def test_a_rule_that_compares_with_an_empty_user_attribute_is_found(self):
        model = self.env.ref("base.model_res_currency")
        strict = self.env["ir.rule"].create(
            {
                "name": "Strict probe",
                "model_id": model.id,
                "domain_force": "[('id', '=', user.action_id.id)]",
                "groups": [(4, self.env.ref("base.group_user").id)],
            }
        )
        careful = strict.copy(
            {
                "name": "Careful probe",
                "domain_force": "[('id', '=', user.action_id.id)] if user.action_id else []",
            }
        )
        unrelated = strict.copy({"name": "Unrelated probe", "domain_force": "[('id', '=', user.id)]"})
        self.assertEqual(personas_module.rule_user_attributes(self.env, strict), {"action_id"})
        self.assertEqual(personas_module.rule_user_attributes(self.env, unrelated), set())
        self.assertTrue(personas_module.rule_empty_attribute_collapse(self.env, strict, "action_id"))
        self.assertFalse(personas_module.rule_empty_attribute_collapse(self.env, careful, "action_id"))
