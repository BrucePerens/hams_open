# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""The Zero Trust policy push as pure data: what pushing Odoo's `cloudflare.zero.trust.policy` rows would do.

No Odoo, no network. Cloudflare keeps reusable Access policies per account, each with its own id, so each
policy is created or updated on its own. A policy is managed when its `name` equals the name of an Odoo row.
Policies Odoo does not manage are never read into the plan beyond their names, and nothing is ever deleted.
"""
import hashlib
import json

CREATE, UPDATE, UNCHANGED = "create", "update", "unchanged"
DECISIONS = {"allow": "allow", "block": "deny", "bypass": "bypass"}
COMPARED_FIELDS = ("decision", "include", "exclude", "require")


# [@ANCHOR: cloudflare:COMM_zero_trust_plan_desired]
def desired_from_row(name, policy_action, idps):
    """One Odoo row as a Cloudflare Access policy, or (None, problem) when it cannot be pushed.

    `idps` is a comma or space separated list of identity-provider ids. An allow policy with no provider
    would let everyone in, so it is refused; a block or bypass policy with none applies to everyone."""
    decision = DECISIONS.get(policy_action)
    if not decision:
        return None, f"row {name!r} has no action"
    ids = [i for i in (idps or "").replace(",", " ").split() if i]
    if ids:
        include = [{"login_method": {"id": i}} for i in ids]
    elif decision == "allow":
        return None, f"row {name!r} allows access but names no identity provider (that would allow everyone)"
    else:
        include = [{"everyone": {}}]
    return {"name": name, "decision": decision, "include": include, "exclude": [], "require": []}, None


# [@ANCHOR: cloudflare:COMM_zero_trust_plan_view]
def _view(policy):
    view = {k: policy.get(k) or ([] if k != "decision" else None) for k in COMPARED_FIELDS}
    view["include"] = sorted(view["include"], key=lambda x: json.dumps(x, sort_keys=True))
    return view


# [@ANCHOR: cloudflare:COMM_zero_trust_plan_make]
def make(remote, desired):
    """Compare the account's policies with Odoo's rows.

    remote: every Access policy of the account as Cloudflare returned it. desired: the dicts of desired_from_row.
    Returns {"writes": [(action, remote id or None, payload)], "entries": [...], "problems": [...], "hash": str}.
    """
    problems, entries, writes = [], [], []
    by_name = {}
    for p in remote:
        by_name.setdefault(p.get("name"), []).append(p)
    seen = set()
    for d in desired:
        if d["name"] in seen:
            problems.append(f"two Odoo rows are named {d['name']!r}; names identify managed policies, so they must differ")
        seen.add(d["name"])
        old = by_name.get(d["name"], [])
        if len(old) > 1:
            problems.append(f"Cloudflare holds two policies named {d['name']!r}; remove one by hand first")
        elif not old:
            writes.append((CREATE, None, d))
            entries.append({"action": CREATE, "name": d["name"], "reason": f"{d['decision']}, {len(d['include'])} include rule(s)"})
        elif _view(old[0]) == _view(d):
            entries.append({"action": UNCHANGED, "name": d["name"], "reason": f"{d['decision']}, {len(d['include'])} include rule(s)"})
        else:
            writes.append((UPDATE, old[0]["id"], d))
            entries.append({"action": UPDATE, "name": d["name"], "reason": f"was {_view(old[0])['decision']}; now {d['decision']}"})
    body = [[(a, i, p) for a, i, p in writes], sorted((p.get("name"), p.get("id")) for p in remote)]
    return {"writes": writes, "entries": entries, "problems": problems,
            "hash": hashlib.sha256(json.dumps(body, sort_keys=True).encode("utf-8")).hexdigest()}


# [@ANCHOR: cloudflare:COMM_zero_trust_plan_render]
def render(plan):
    order = {CREATE: 0, UPDATE: 1, UNCHANGED: 2}
    lines = [f"{e['action'].upper():9} {e['name']}  ({e['reason']})" for e in sorted(plan["entries"], key=lambda e: (order[e["action"]], e["name"]))]
    lines.append(f"Summary: {len(plan['writes'])} policy write(s); policies Odoo does not manage are never touched or deleted.")
    return "\n".join(lines)
