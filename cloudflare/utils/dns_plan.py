# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""The DNS push as pure data: what a push of Odoo's `cloudflare.dns.record` rows would do.

No Odoo, no network. The model reads Cloudflare (GET only), hands this module the rows and what it
found, and gets a plan back, the way `cloudflare.tunnel._build_ingress()` computes the tunnel list
before anything is sent. Only the actions `create` and `update` write to Cloudflare; `adopt` writes
the Cloudflare record id into the Odoo row and nothing at Cloudflare. `delete` exists for one case only: a row an administrator
marked "retire" that is linked to a Cloudflare record by id, and whose record at Cloudflare is still
that same type and name. A record Odoo has no row for is never touched, and removing or archiving an
Odoo row on its own removes nothing at Cloudflare.
"""
import hashlib
import ipaddress
import json

# [@ANCHOR: cloudflare:COMM_dns_plan]
CREATE, UPDATE, DELETE, ADOPT, UNCHANGED, DRIFT, CONFLICT, PROBLEM = (
    "create", "update", "delete", "adopt", "unchanged", "drift", "conflict", "problem",
)
# Only these touch Cloudflare.
WRITING_ACTIONS = frozenset({CREATE, UPDATE, DELETE})
# Types that may hold several values under one name (a nameserver set, several TXT strings).
MULTI_VALUE_TYPES = frozenset({"NS", "TXT"})
# Cloudflare can proxy only these; a proxied NS or TXT record is refused by the API.
PROXIABLE_TYPES = frozenset({"A", "AAAA", "CNAME"})


def normalize_name(name):
    return (name or "").strip().rstrip(".").lower()


def normalize_content(record_type, content):
    """The form two spellings of the same value share, so Odoo and Cloudflare can be compared."""
    text = (content or "").strip()
    if record_type in ("NS", "CNAME"):
        return text.rstrip(".").lower()
    if record_type == "AAAA":
        try:
            return ipaddress.IPv6Address(text).compressed
        except ValueError:
            return text.lower()
    if record_type == "TXT":
        # Cloudflare returns TXT content wrapped in double quotes.
        if len(text) >= 2 and text[0] == text[-1] == '"':
            text = text[1:-1]
        return text
    return text


def effective_proxied(record_type, proxied):
    return bool(proxied) and record_type in PROXIABLE_TYPES


def _entry(action, row, reason="", **extra):
    entry = {"action": action, "row_id": row["id"], "type": row["type"], "name": row["name"],
             "content": row["content"], "reason": reason,
             "proxied": effective_proxied(row["type"], row["proxied"])}
    entry.update(extra)
    return entry


def plan_row(row, remote):
    """One row against what Cloudflare holds under the same name.

    row: {id, name, type, content, proxied, manage, cf_record_id}.
    remote: every record Cloudflare has under that exact name (all types), as the API returned them.
    """
    kind = row["type"]
    want_content = normalize_content(kind, row["content"])
    want_proxied = effective_proxied(kind, row["proxied"])

    def view(rec):
        return (normalize_content(rec.get("type"), rec.get("content")), bool(rec.get("proxied")))

    if row.get("retire"):
        return _plan_retire(row, remote)

    if row.get("cf_record_id"):
        mine = [r for r in remote if r.get("id") == row["cf_record_id"]]
        if not mine:
            return _entry(CONFLICT, row, "the linked Cloudflare record id is not under this name at Cloudflare "
                          "(renamed or deleted there); nothing was changed. Clear the id to create it again")
        rec = mine[0]
        if rec.get("type") != kind:
            return _entry(CONFLICT, row, f"the linked record is a {rec.get('type')} record, this row is {kind}; "
                          "a record's type is never changed, make a new row")
        have_content, have_proxied = view(rec)
        if (have_content, have_proxied) == (want_content, want_proxied):
            return _entry(UNCHANGED, row, "already as Odoo has it", remote_id=rec["id"])
        diff = []
        if have_content != want_content:
            diff.append(f"content {have_content!r} -> {want_content!r}")
        if have_proxied != want_proxied:
            diff.append(f"proxied {have_proxied} -> {want_proxied}")
        if not row.get("manage"):
            return _entry(DRIFT, row, "observe only, not changed: " + "; ".join(diff), remote_id=rec["id"])
        return _entry(UPDATE, row, "; ".join(diff), remote_id=rec["id"], ttl=rec.get("ttl", 1))

    same_type = [r for r in remote if r.get("type") == kind]
    exact = [r for r in same_type if normalize_content(kind, r.get("content")) == want_content]
    if exact:
        rec = exact[0]
        if bool(rec.get("proxied")) != want_proxied:
            return _entry(CONFLICT, row, f"a matching {kind} record exists with proxied={bool(rec.get('proxied'))}, "
                          f"Odoo says {want_proxied}; adopting never changes Cloudflare, so set the row to match "
                          "what Cloudflare has (or take the record over by pasting its id into the row)")
        return _entry(ADOPT, row, "same record already exists at Cloudflare; only its id is stored in Odoo",
                      remote_id=rec["id"])
    if not row.get("manage"):
        return _entry(DRIFT, row, "observe only: no matching record at Cloudflare, nothing created")
    if kind not in MULTI_VALUE_TYPES and same_type:
        have = ", ".join(sorted(normalize_content(kind, r.get("content")) for r in same_type))
        return _entry(CONFLICT, row, f"a {kind} record already exists under this name with other content ({have}); "
                      "Odoo never overwrites a record it did not create. Paste that record's id into the row to "
                      "take it over, or change the row to match")
    # A CNAME stands alone: it cannot share a name with any other record type.
    cname_clash = [r for r in remote if (r.get("type") == "CNAME") != (kind == "CNAME")]
    if cname_clash:
        return _entry(CONFLICT, row, f"a {cname_clash[0].get('type')} record under this name cannot sit beside "
                      f"a {kind} record (a CNAME stands alone)")
    note = ""
    if same_type:
        note = f"adds a value next to {len(same_type)} existing {kind} record(s), none of them changed"
    return _entry(CREATE, row, note)


def _plan_retire(row, remote):
    """A row marked retire: delete its own record at Cloudflare, and nothing else."""
    kind = row["type"]
    if not row.get("cf_record_id"):
        return _entry(UNCHANGED, row, "marked retire but never linked to a Cloudflare record: nothing to delete")
    if not row.get("manage"):
        return _entry(DRIFT, row, "marked retire but observe only: nothing is deleted")
    mine = [r for r in remote if r.get("id") == row["cf_record_id"]]
    if not mine:
        return _entry(UNCHANGED, row, "marked retire and already absent at Cloudflare", already_gone=True)
    rec = mine[0]
    if rec.get("type") != kind:
        return _entry(CONFLICT, row, f"marked retire, but the linked record is a {rec.get('type')} record, not {kind}; "
                      "nothing was deleted")
    return _entry(DELETE, row, "marked retire: this record is removed at Cloudflare", remote_id=rec["id"])


def plan_hash(entries):
    """A digest of the writing part of a plan, so that applying can insist on exactly what was shown."""
    body = [
        [e["action"], e["row_id"], e["type"], e["name"], normalize_content(e["type"], e["content"]),
         e.get("remote_id", ""), e.get("proxied"), e.get("zone_id", "")]
        for e in entries if e["action"] in WRITING_ACTIONS or e["action"] == ADOPT or e.get("already_gone")
    ]
    return hashlib.sha256(json.dumps(body, sort_keys=True).encode("utf-8")).hexdigest()


def render(entries):
    """The plan as text for a person: one line per row, writing actions first."""
    order = {CREATE: 0, UPDATE: 1, DELETE: 2, ADOPT: 3, CONFLICT: 4, PROBLEM: 5, DRIFT: 6, UNCHANGED: 7}
    lines = []
    for e in sorted(entries, key=lambda e: (order[e["action"]], e["name"], e["type"])):
        tail = f"  ({e['reason']})" if e.get("reason") else ""
        lines.append(f"{e['action'].upper():9} {e['type']:5} {e['name']} -> {e['content']}{tail}")
    counts = {}
    for e in entries:
        counts[e["action"]] = counts.get(e["action"], 0) + 1
    lines.append("Summary: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())))
    lines.append("Records at Cloudflare that Odoo has no row for are never changed or deleted; a delete happens only "
                 "for a row marked retire that is linked to the record by id.")
    return "\n".join(lines)
