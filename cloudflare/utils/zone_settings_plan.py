# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""The zone-settings push as pure data: what pushing Odoo's `cloudflare.zone.settings` row would change.

No Odoo, no network. The caller reads the zone's current value of every setting the Odoo row sets, this
module compares them with what the row wants, and says which settings would be written. A setting the row
leaves unset (an empty SSL mode) is not in `desired` and is never touched.
"""
import hashlib
import json

UPDATE, UNCHANGED = "update", "unchanged"
MINIFY_PARTS = ("css", "html", "js")


# [@ANCHOR: cloudflare:COMM_zone_settings_plan_desired]
def desired_from_row(ssl_mode, auto_minify, bot_fight_mode):
    """The settings one Odoo row wants, keyed by Cloudflare setting: ssl, minify, bot_fight_mode."""
    desired = {}
    if ssl_mode:
        desired["ssl"] = ssl_mode
    onoff = "on" if auto_minify else "off"
    desired["minify"] = {part: onoff for part in MINIFY_PARTS}
    desired["bot_fight_mode"] = bool(bot_fight_mode)
    return desired


# [@ANCHOR: cloudflare:COMM_zone_settings_plan_make]
def make(remote, desired):
    """Compare the zone's current values with the wanted ones.

    remote: {setting: current value} for every key of `desired` (the caller refuses when one is unreadable).
    Returns {"changes": {setting: new value}, "entries": [...], "hash": str}.
    """
    changes, entries = {}, []
    for key in sorted(desired):
        want, have = desired[key], remote.get(key)
        if want == have:
            entries.append({"action": UNCHANGED, "name": key, "reason": f"already {_show(want)}"})
        else:
            changes[key] = want
            entries.append({"action": UPDATE, "name": key, "reason": f"was {_show(have)}; now {_show(want)}"})
    body = [changes, {k: remote.get(k) for k in sorted(desired)}]
    return {"changes": changes, "entries": entries, "hash": hashlib.sha256(json.dumps(body, sort_keys=True).encode("utf-8")).hexdigest()}


# [@ANCHOR: cloudflare:COMM_zone_settings_plan_show]
def _show(value):
    if isinstance(value, dict):
        return ", ".join(f"{k} {value[k]}" for k in sorted(value))
    return str(value)


# [@ANCHOR: cloudflare:COMM_zone_settings_plan_render]
def render(plan):
    lines = [f"{e['action'].upper():9} {e['name']}  ({e['reason']})" for e in plan["entries"]]
    lines.append(f"Summary: {len(plan['changes'])} zone setting(s) would change; nothing else is touched.")
    return "\n".join(lines)
