# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""The cache-rule push as pure data: what pushing Odoo's `cloudflare.cache.rule` rows would do.

No Odoo, no network. Cloudflare keeps ONE cache-settings ruleset per zone and the API replaces all of it
on a write. So the push is planned first: the zone's current rules are read, the rules Odoo manages are
merged into them, and every rule Odoo does not manage is carried through untouched.

A rule is managed when its `description` equals the `name` of an Odoo cache-rule row (active or not).
Everything else in the zone's ruleset (a dashboard rule, a rule another tool made) is unmanaged and is
never altered, never deleted and never reordered relative to the other unmanaged rules. The merged list
is: managed bypass rules (so no cache rule is ever live without the bypass in front of it, decision 112),
then the unmanaged rules in their original order, then the managed cache rules.

`merge()` proves its own result: it refuses the plan (a `problems` entry) if the unmanaged rules in the
result are not exactly the unmanaged rules it was given.
"""
import hashlib
import json

KEEP, CREATE, UPDATE, DELETE, UNCHANGED = "keep", "create", "update", "delete", "unchanged"
# Fields Cloudflare returns but does not accept back on a write.
READ_ONLY_FIELDS = ("version", "last_updated", "categories")
# The fields that decide what a managed rule does.
COMPARED_FIELDS = ("action", "action_parameters", "expression", "description", "enabled")


def clean(rule):
    """A remote rule as it can be sent back unchanged."""
    return {k: v for k, v in rule.items() if k not in READ_ONLY_FIELDS}


def _view(rule):
    return {k: rule.get(k) for k in COMPARED_FIELDS}


def merge(remote_rules, desired, managed_names):
    """Merge Odoo's rules into the zone's current rules.

    remote_rules: the zone ruleset's current `rules` (empty list when the ruleset does not exist yet).
    desired: Odoo's active rules in push order, each {action, action_parameters, expression, description,
             enabled}; a bypass rule is one whose action_parameters is {"cache": False}.
    managed_names: every Odoo cache-rule name, active or not.
    Returns {"rules": [...], "entries": [...], "problems": [...], "hash": str}.
    """
    problems, entries = [], []
    managed_names = set(managed_names)
    desired_by_name = {}
    for d in desired:
        if d["description"] in desired_by_name:
            problems.append(f"two Odoo rows are named {d['description']!r}; names identify managed rules, so they must differ")
        desired_by_name[d["description"]] = d
    remote_managed, unmanaged, seen = {}, [], set()
    for r in remote_rules:
        name = r.get("description")
        if name in managed_names:
            if name in seen:
                problems.append(f"Cloudflare holds two rules described {name!r}; remove one by hand first")
            seen.add(name)
            remote_managed[name] = r
        else:
            unmanaged.append(clean(r))
    new_bypass, new_cache = [], []
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
                entries.append({"action": UPDATE, "name": d["description"],
                                "reason": f"was: {_summary(old)}; now: {_summary(d)}"})
        (new_bypass if d["action_parameters"] == {"cache": False} else new_cache).append(rule)
    for name, old in remote_managed.items():
        if name not in desired_by_name:
            entries.append({"action": DELETE, "name": name, "reason": "an Odoo row of this name exists but is inactive"})
    for u in unmanaged:
        entries.append({"action": KEEP, "name": u.get("description") or u.get("id", "?"), "reason": "not managed by Odoo; left as is"})
    merged = new_bypass + unmanaged + new_cache
    # The proof: the unmanaged rules in the result are exactly the ones given, in order.
    kept = [r for r in merged if r.get("description") not in managed_names]
    if kept != unmanaged or len(kept) != sum(1 for r in remote_rules if r.get("description") not in managed_names):
        problems.append("internal check failed: the merge would change a rule Odoo does not manage")
    return {"rules": merged, "entries": entries, "problems": problems, "hash": plan_hash(merged, remote_rules)}


def _summary(rule):
    params = rule.get("action_parameters") or {}
    if params.get("cache") is False:
        what = "bypass cache"
    else:
        edge = params.get("edge_ttl") or {}
        browser = params.get("browser_ttl") or {}
        what = f"cache, edge {edge.get('mode', 'default')}" + (f" {edge['default']}s" if edge.get("default") else "")
        what += f", browser {browser.get('mode', 'zone default')}"
    return f"{what} when {rule.get('expression')}"


def plan_hash(merged, remote_rules):
    """A digest of what Apply would write and what it was computed from, so applying insists on the reviewed plan."""
    body = [merged, [clean(r) for r in remote_rules]]
    return hashlib.sha256(json.dumps(body, sort_keys=True).encode("utf-8")).hexdigest()


def render(plan, ruleset_state):
    order = {CREATE: 0, UPDATE: 1, DELETE: 2, KEEP: 3, UNCHANGED: 4}
    lines = [f"Zone cache-settings ruleset: {ruleset_state}"]
    for e in sorted(plan["entries"], key=lambda e: (order[e["action"]], e["name"])):
        lines.append(f"{e['action'].upper():9} {e['name']}  ({e['reason']})")
    lines.append("Resulting order: " + " | ".join(r.get("description") or r.get("id", "?") for r in plan["rules"]))
    writes = sum(1 for e in plan["entries"] if e["action"] in (CREATE, UPDATE, DELETE))
    lines.append(f"Summary: {writes} change(s) to managed rules; unmanaged rules are never altered, deleted or reordered.")
    return "\n".join(lines)
