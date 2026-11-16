# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""The rate-limit push as pure data: what pushing Odoo's `cloudflare.rate.limit` rows would do.

No Odoo, no network. Cloudflare keeps ONE rate-limiting ruleset per zone (phase http_ratelimit) and a write
replaces all of it, so the push is planned first: the zone's current rules are read, the rules Odoo manages
are merged in, and every rule Odoo does not manage is carried through untouched.

A rule is managed when its `description` equals the `name` of an Odoo rate-limit row (active or not). The
merged list is the unmanaged rules in their original order, then the managed rules in Odoo's order.
`merge()` proves its own result: it reports a problem if the unmanaged rules in the result are not exactly
the ones it was given.
"""
import hashlib
import json

from . import cache_plan

CREATE, UPDATE, DELETE, KEEP, UNCHANGED = "create", "update", "delete", "keep", "unchanged"
COMPARED_FIELDS = ("action", "expression", "description", "enabled")
RATELIMIT_FIELDS = ("characteristics", "period", "requests_per_period", "mitigation_timeout")


# [@ANCHOR: cloudflare:COMM_rate_limit_plan_view]
def _view(rule):
    view = {k: rule.get(k) for k in COMPARED_FIELDS}
    limit = rule.get("ratelimit") or {}
    view["ratelimit"] = {k: limit.get(k) for k in RATELIMIT_FIELDS}
    return view


# [@ANCHOR: cloudflare:COMM_rate_limit_plan_summary]
def _summary(rule):
    limit = rule.get("ratelimit") or {}
    return (f"{rule.get('action')} above {limit.get('requests_per_period')} request(s) per {limit.get('period')}s "
            f"for {limit.get('mitigation_timeout')}s when {rule.get('expression')}")


# [@ANCHOR: cloudflare:COMM_rate_limit_plan_merge]
def merge(remote_rules, desired, managed_names):
    """Merge Odoo's rate-limit rules into the zone's current rules.

    desired: Odoo's active rules, each {action, expression, description, enabled, ratelimit}.
    managed_names: every Odoo rate-limit name, active or not.
    Returns {"rules", "entries", "problems", "hash"}.
    """
    problems, entries = [], []
    managed_names = set(managed_names)
    desired_names = set()
    for d in desired:
        if d["description"] in desired_names:
            problems.append(f"two Odoo rows are named {d['description']!r}; names identify managed rules, so they must differ")
        desired_names.add(d["description"])
    remote_managed, unmanaged, seen = {}, [], set()
    for r in remote_rules:
        name = r.get("description")
        if name in managed_names:
            if name in seen:
                problems.append(f"Cloudflare holds two rules described {name!r}; remove one by hand first")
            seen.add(name)
            remote_managed[name] = r
        else:
            unmanaged.append(cache_plan.clean(r))
    managed = []
    for d in desired:
        rule = dict(d)
        old = remote_managed.get(d["description"])
        if old is None:
            entries.append({"action": CREATE, "name": d["description"], "reason": _summary(d)})
        else:
            rule["id"] = old["id"]
            if _view(old) == _view(rule):
                entries.append({"action": UNCHANGED, "name": d["description"], "reason": _summary(d)})
            else:
                entries.append({"action": UPDATE, "name": d["description"], "reason": f"was: {_summary(old)}; now: {_summary(d)}"})
        managed.append(rule)
    for name in remote_managed:
        if name not in desired_names:
            entries.append({"action": DELETE, "name": name, "reason": "an Odoo row of this name exists but is inactive"})
    for u in unmanaged:
        entries.append({"action": KEEP, "name": u.get("description") or u.get("id", "?"), "reason": "not managed by Odoo; left as is"})
    merged = unmanaged + managed
    kept = [r for r in merged if r.get("description") not in managed_names]
    if kept != unmanaged:
        problems.append("internal check failed: the merge would change a rule Odoo does not manage")
    body = [merged, [cache_plan.clean(r) for r in remote_rules]]
    digest = hashlib.sha256(json.dumps(body, sort_keys=True).encode("utf-8")).hexdigest()
    return {"rules": merged, "entries": entries, "problems": problems, "hash": digest}


# [@ANCHOR: cloudflare:COMM_rate_limit_plan_render]
def render(plan, ruleset_state):
    order = {CREATE: 0, UPDATE: 1, DELETE: 2, KEEP: 3, UNCHANGED: 4}
    lines = [f"Zone rate-limiting ruleset: {ruleset_state}"]
    for e in sorted(plan["entries"], key=lambda e: (order[e["action"]], e["name"])):
        lines.append(f"{e['action'].upper():9} {e['name']}  ({e['reason']})")
    writes = sum(1 for e in plan["entries"] if e["action"] in (CREATE, UPDATE, DELETE))
    lines.append(f"Summary: {writes} change(s) to managed rules; unmanaged rules are never altered or deleted.")
    return "\n".join(lines)
