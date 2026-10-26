# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Test personas: the users of hams.com, as production has them.

A test that runs as the superuser, or as a user invented for the test, shows only that the code works for someone
nobody is. Two incidents in one week got past a green suite that way: a cron whose service account could not write the
model of its own server action, and an admin record rule that compared a website with `user.website_id.id` when every
internal admin has no website. This module holds the catalogue of production users, the choice of which of them a test
needs, and a runner that executes a test body once for each.

Every fact in CATALOGUE was read from the production database on 2026-10-26 (counts, group names and the `website_id`
of users only). Two different things are called a website: Odoo's `res.users.website_id` (empty for all 279 active users, so
`user.website_id.id` in a rule is False for every one of them) and the member's own site from user_websites
(`website_slug`; 264 of the 279 have one). A persona has the second where that module is installed and not the first.
"""

import functools
import itertools
import os
import re

from odoo.exceptions import AccessError, ValidationError
from odoo.modules.module import get_module_path
from odoo.tools.safe_eval import safe_eval

_persona_counter = itertools.count(1)


class PersonaError(AssertionError):
    """A test asked for a persona that cannot be built or chosen. An AssertionError so the test fails, never skips."""


# What production has, by kind. `groups` are xmlids; `website` is "none" or "own" (the value of user.website_id).
# `prod` is what was counted.
CATALOGUE = {
    "public": {
        "kind": "public",
        "existing_user": "base.public_user",
        "prod": "the single public user every visitor is served as",
    },
    "portal_member": {
        "kind": "portal",
        "groups": ("base.group_portal", "user_websites.group_user_websites_user"),
        "website": "none",
        "prod": "143 active members, user.website_id empty for all; most have their own member site (website_slug)",
    },
    "internal_staff": {
        "kind": "internal",
        "groups": ("base.group_user",),
        "website": "none",
        "prod": "14 internal users, none with res.users.website_id; 3 are people, the rest are service accounts",
    },
    "admin": {
        "kind": "internal",
        "groups": ("base.group_user", "base.group_system"),
        "website": "none",
        "prod": "2 administrators, neither bound to a website",
    },
    "pager_admin": {
        "kind": "internal",
        "groups": ("base.group_user", "pager_duty.group_pager_admin"),
        "website": "none",
        "prod": "2 pager admins, neither bound to a website",
    },
    "helpdesk_staff": {
        "kind": "internal",
        "groups": ("base.group_user", "hams_helpdesk.group_helpdesk_manager"),
        "website": "none",
        "prod": "3 helpdesk managers, none bound to a website",
    },
}

# hams_open is open source and meant for multi-tenant installs, where a login is bound to its website (per-website
# accounts, `specific_user_account`). hams.com binds none, so every portal persona also exists bound to a website of its
# own: `<key>_with_website`. A rule or route that works for one and not the other is a bug in one deployment. Odoo
# refuses to bind an internal user to a website ("Remove website on related partner before they become internal user"),
# so internal personas and admins exist only unbound.
for _key, _spec in list(CATALOGUE.items()):
    if _spec.get("kind") == "portal":
        CATALOGUE[f"{_key}_with_website"] = dict(
            _spec,
            website="own",
            prod="not on hams.com (no login is bound to a website); an install with per-website accounts",
        )

# A service account is a data record shipped by its module, never a copy: production runs exactly that user (258 of
# the 267 portal-type users are service accounts or members; a few services, such as the event and pager services, are
# internal users). Ask for one with service(xmlid); a cron runs as its own user_id, ask with cron(xmlid or record).


class Persona:
    """One user of the site, with the environment to act as them and what the test expects of them."""

    # [@ANCHOR: zero_sudo:persona_object]
    def __init__(self, key, user, password=None, allowed=None):
        self.key = key
        self.user = user
        self.password = password
        self.allowed = allowed
        self.env = user.with_user(user).env

    def __repr__(self):
        return f"Persona({self.key}, login={self.user.login}, allowed={self.allowed})"


def _group(env, xmlid):
    group = env.ref(xmlid, raise_if_not_found=False)
    if not group:
        raise PersonaError(f"Persona needs group {xmlid}, which is not installed; install its module in this run.")
    return group


# [@ANCHOR: zero_sudo:persona_build]
def build_persona(env, key, allowed=None, extra_group=None):
    """Create the catalogue persona `key` (or the existing public user), as production has it."""
    spec = CATALOGUE.get(key)
    if spec is None:
        raise PersonaError(f"No persona '{key}'. Catalogue: {sorted(CATALOGUE)}")
    if "existing_user" in spec:
        return Persona(key, env.ref(spec["existing_user"]), allowed=allowed)
    groups = [_group(env, xmlid) for xmlid in spec["groups"]]
    if extra_group:
        groups.append(extra_group)
    number = next(_persona_counter)
    password = f"pw-{key}-{number}-{id(env)}"
    values = {
        "name": f"Persona {key} {number}",
        "login": f"persona_{key}_{number}",
        "password": password,
        "group_ids": [(6, 0, [group.id for group in groups])],
        "website_id": False,
    }
    if spec["website"] == "own":
        values["website_id"] = env["website"].create({"name": f"Persona website {number}"}).id
    if "website_slug" in env["res.users"]._fields:
        # Every human user in production has a member site; the slug is lowercase letters, digits and hyphens.
        values["website_slug"] = f"persona-{key.replace('_', '-')}-{number}"
    user = env["res.users"].with_context(no_reset_password=True).create(values)
    # Odoo fills in a default website for a new user on some installs; production users have none.
    if spec["website"] == "none" and user.website_id:
        user.write({"website_id": False})
    return Persona(key, user, password=password, allowed=allowed)


# [@ANCHOR: zero_sudo:persona_service]
def service(env, xmlid, allowed=None):
    """The service account `xmlid` exactly as its module installs it (its real groups, website and share flag)."""
    user = env.ref(xmlid, raise_if_not_found=False)
    if not user or user._name != "res.users":
        raise PersonaError(f"No service account {xmlid} in this database.")
    return Persona(f"service:{xmlid}", user, allowed=allowed)


# [@ANCHOR: zero_sudo:persona_cron]
def cron(env, cron_ref, allowed=None):
    """The user a cron runs as, from the cron record or its xmlid."""
    record = env.ref(cron_ref) if isinstance(cron_ref, str) else cron_ref
    return Persona(f"cron:{record.cron_name}", record.user_id, allowed=allowed)


# [@ANCHOR: zero_sudo:persona_run_cron_action]
def run_cron_action(env, cron_ref):
    """Run the server action of a cron as the cron's own user, the way ir.cron does (checks included).

    The model method alone is not enough: ir.actions.server.run() first refuses a user without write access to the
    action's model, so a method called directly passes where the cron fails."""
    record = env.ref(cron_ref) if isinstance(cron_ref, str) else cron_ref
    return record.ir_actions_server_id.with_user(record.user_id).run()


# [@ANCHOR: zero_sudo:persona_cron_refusal]
def cron_refusal(cron_record):
    """The AccessError text Odoo would raise before running this cron's server action as its user, or None.

    Calls the exact gate ir.actions.server.run() calls (`_can_execute_action_on_records`) with the empty recordset run()
    gives it. It does not run the action: a cron may fetch from a third-party server, which a test machine must not do."""
    action = cron_record.ir_actions_server_id
    user = cron_record.user_id
    model = action.model_name
    try:
        action.with_user(user)._can_execute_action_on_records(action.env[model].with_user(user))
    except AccessError as error:
        return str(error)
    return None


# [@ANCHOR: zero_sudo:persona_route_derivation]
def route_personas(env, auth, groups=()):
    """Personas for a controller route: who may use it and who must be refused, from `auth=` and the groups it checks."""
    if auth == "none":
        return [build_persona(env, "public", allowed=True)]
    if auth == "public":
        keys = ["public", "portal_member", "internal_staff", "admin"]
        return [build_persona(env, key, allowed=True) for key in keys]
    if auth not in ("user", "bearer"):
        raise PersonaError(f"Route auth '{auth}' has no persona rule.")
    result = [build_persona(env, "public", allowed=False)]
    named = [_group(env, xmlid) for xmlid in groups]
    for key in ("portal_member", "internal_staff", "admin"):
        persona = build_persona(env, key)
        persona.allowed = (not named) or any(group in persona.user.all_group_ids for group in named)
        result.append(persona)
    return result


# [@ANCHOR: zero_sudo:persona_model_derivation]
def model_personas(env, model_name, mode="read"):
    """Personas for a model: a user in each group its ACL rows and record rules name, and one who must be refused."""
    perm = f"perm_{mode}"
    named = env["res.groups"]
    unconditional = False
    accesses = env["ir.model.access"].search([("model_id.model", "=", model_name), (perm, "=", True)])
    for access in accesses:
        if access.group_id:
            named |= access.group_id
        else:
            unconditional = True
    rules = env["ir.rule"].search([("model_id.model", "=", model_name), (perm, "=", True)])
    for rule in rules:
        named |= rule.groups
    result = []
    covered = env["res.groups"]
    for key, spec in CATALOGUE.items():
        if "groups" not in spec:
            continue
        held = [env.ref(xmlid, raise_if_not_found=False) for xmlid in spec["groups"]]
        if not all(held):
            continue
        persona = build_persona(env, key)
        match = persona.user.all_group_ids & named
        if match:
            persona.allowed = True
            covered |= match
            result.append(persona)
    for group in named - covered:
        # A group no catalogue persona holds: an internal user holding exactly that group, no website.
        persona = build_persona(env, "internal_staff", extra_group=group)
        persona.key = f"internal_staff+{group.full_name}"
        persona.allowed = True
        result.append(persona)
    present = {persona.key for persona in result}
    for key in ("public", "portal_member", "internal_staff"):
        if key in present:
            continue
        persona = build_persona(env, key)
        persona.allowed = bool(unconditional) or bool(persona.user.all_group_ids & named)
        if not persona.allowed:
            result.append(persona)
    if not named and not unconditional:
        raise PersonaError(f"{model_name} has no ACL row for {mode}: no persona can be derived. Name them explicitly.")
    return result


# [@ANCHOR: zero_sudo:persona_decorator]
def personas(*keys, model=None, mode="read", cron_ref=None, route_auth=None, route_groups=(), services=()):
    """Run the decorated test body once per persona: `def test_x(self, persona)`.

    Name personas by catalogue key, `services=` (service account xmlids), `cron_ref=` (its user), `model=`/`mode=` (ACL
    derived) or `route_auth=`/`route_groups=` (derived from the route). A test naming none fails, so nothing ever runs
    as the superuser by omission. Each persona runs in its own savepoint, rolled back, and its own subTest."""
    if not (keys or model or cron_ref or route_auth or services):
        raise PersonaError("@personas() names no persona; name keys, model=, cron_ref=, route_auth= or services=.")

    def decorate(test):
        @functools.wraps(test)
        def wrapper(self, *args, **kwargs):
            env = self.env
            chosen = [build_persona(env, key, allowed=True) for key in keys]
            chosen += [service(env, xmlid, allowed=True) for xmlid in services]
            if cron_ref:
                chosen.append(cron(env, cron_ref, allowed=True))
            if model:
                chosen += model_personas(env, model, mode)
            if route_auth:
                chosen += route_personas(env, route_auth, route_groups)
            if not chosen:
                raise PersonaError(f"{test.__name__}: no persona derived; the test would run as nobody.")
            for persona in chosen:
                with self.subTest(persona=persona.key):
                    self._run_in_rolled_back_savepoint(test, persona, *args, **kwargs)

        return wrapper

    return decorate


class _Rollback(Exception):
    pass


class PersonaMixin:
    """Persona helpers for HamsTransactionCase and HamsHttpCase."""

    # [@ANCHOR: zero_sudo:persona_savepoint_runner]
    def _run_in_rolled_back_savepoint(self, test, persona, *args, **kwargs):
        try:
            with self.env.cr.savepoint():
                test(self, persona, *args, **kwargs)
                raise _Rollback()
        except _Rollback:
            self.env.invalidate_all()

    # [@ANCHOR: zero_sudo:persona_assert_access]
    def assert_persona_access(self, persona, records, mode="read"):
        """The persona can (allowed) or cannot (refused) `mode` the records, as the declaration says."""
        has = bool(records.with_user(persona.user).has_access(mode))
        self.assertEqual(has, bool(persona.allowed), f"{persona.key} {mode} {records}: allowed={persona.allowed}")

    # [@ANCHOR: zero_sudo:persona_login]
    def authenticate_as(self, persona):
        """Log the HTTP session in as the persona (a catalogue persona has a known password)."""
        if persona.password is None:
            raise PersonaError(f"{persona.key} has no password; only built catalogue personas can log in.")
        return self.authenticate(persona.user.login, persona.password)


_USER_ATTRIBUTE = re.compile(r"\buser\.([a-z_][a-z0-9_]*)")


# [@ANCHOR: zero_sudo:persona_rule_attributes]
def rule_user_attributes(env, rule):
    """The optional fields of res.users that the rule's domain reads, e.g. {'website_id'}; the ones that can be empty."""
    names = set(_USER_ATTRIBUTE.findall(rule.domain_force or ""))
    fields = env["res.users"]._fields
    return {name for name in names if name in fields and name != "id" and not fields[name].required}


def _empty(value):
    return value is False or value is None or value == [] or value == ()


def _probe_user(env, rule, login, values):
    """A user in the rule's groups (a user type group is added if the rule names none), or None if no such user exists.

    A rule that names the portal and public groups together cannot be held by one user, so each group is tried alone."""
    groups = rule.groups or _group(env, "base.group_user")
    portal = _group(env, "base.group_portal")
    if portal not in groups | groups.all_implied_ids:
        groups |= _group(env, "base.group_user")
    for candidate in [groups] + [group for group in groups]:
        try:
            with env.cr.savepoint():
                values = {"website_id": False, **values}
                if "website_slug" in env["res.users"]._fields:
                    values["website_slug"] = login.replace("_", "-")
                return env["res.users"].with_context(no_reset_password=True).create(
                    {"name": f"Rule probe {login}", "login": login, "group_ids": [(6, 0, candidate.ids)], **values}
                )
        except ValidationError:
            continue
    return None


def _evaluate(rule, user):
    context = rule.with_user(user)._eval_context()
    return safe_eval(rule.domain_force, context)


# [@ANCHOR: zero_sudo:persona_rule_empty_attribute]
def rule_empty_attribute_collapse(env, rule, attribute):
    """True when the rule, for a user whose `attribute` is empty, tests records against the empty value.

    That is the website bug: `('website_id', '=', user.website_id.id)` becomes `('website_id', '=', False)`, so the user
    sees only records with no website. The domain is evaluated for a user with the attribute empty and for one with it
    set; a leaf that holds a value in the second and an empty one in the first is the collapse. A domain that changes
    shape (the fixed rule adds the test only `if user.website_id`) is null-safe. Returns None when no user with the
    attribute set can be made (a relation with no record to point at)."""
    field = env["res.users"]._fields[attribute]
    if field.type not in ("many2one", "char"):
        return None
    number = next(_persona_counter)
    target = None
    if field.type == "many2one":
        target = env[field.comodel_name].search([], limit=1)
        if not target and field.comodel_name == "website":
            target = env["website"].create({"name": "Rule probe website"})
        if not target:
            return None
    value = target.id if target else "probe"
    empty_user = _probe_user(env, rule, f"rule_probe_empty_{number}", {attribute: False})
    full_user = _probe_user(env, rule, f"rule_probe_full_{number}", {attribute: value})
    if not empty_user or not full_user:
        return None
    empty_domain = _evaluate(rule, empty_user)
    full_domain = _evaluate(rule, full_user)
    if len(empty_domain) != len(full_domain):
        return False
    for empty_leaf, full_leaf in zip(empty_domain, full_domain):
        if isinstance(empty_leaf, tuple) and isinstance(full_leaf, tuple) and len(full_leaf) == 3:
            if _empty(empty_leaf[2]) and not _empty(full_leaf[2]):
                return True
    return False


# [@ANCHOR: zero_sudo:persona_repository_modules]
def in_repository(module, member):
    """True when `module` lives in the same repository (directory of addons) as the module `member`."""
    path = get_module_path(module, display_warning=False)
    return bool(path) and os.path.dirname(path) == os.path.dirname(get_module_path(member))


# [@ANCHOR: zero_sudo:persona_cron_sweep]
def cron_sweep(env, repository=None):
    """Every ir.cron of the installed modules (or of those in the repository of module `repository`) whose own user
    would be refused, as readable lines."""
    rows = env["ir.model.data"].search([("model", "=", "ir.cron")])
    refused = []
    for row in rows:
        if repository is not None and not in_repository(row.module, repository):
            continue
        record = env["ir.cron"].with_context(active_test=False).browse(row.res_id).exists()
        if not record:
            continue
        reason = cron_refusal(record)
        if reason:
            model = record.ir_actions_server_id.model_name
            refused.append(f"{row.module}.{row.name} (as {record.user_id.login} on {model}): {reason}")
    return refused


# [@ANCHOR: zero_sudo:persona_rule_sweep]
def rule_sweep(env, repository):
    """{rule xmlid: [attributes]} of the record rules in the repository of module `repository` that collapse for a user
    with that attribute empty."""
    found = {}
    rows = env["ir.model.data"].search([("model", "=", "ir.rule")])
    for row in rows:
        if not in_repository(row.module, repository):
            continue
        rule = env["ir.rule"].browse(row.res_id).exists()
        if not rule:
            continue
        for attribute in sorted(rule_user_attributes(env, rule)):
            if rule_empty_attribute_collapse(env, rule, attribute):
                found.setdefault(f"{row.module}.{row.name}", []).append(attribute)
    return found
