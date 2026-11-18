# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Which secret-named environment variables a process still carries (read from /proc/<pid>/environ).

A secret in the process environment is readable by anything that reads the process's environment, so a host
that moved a group to the secret store (hams_module_secrets_from_store) must not still load the old variable
into the web unit. Plain Python, no `odoo` import. Only names are ever returned, never a value. Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets"). HAMS_CRYPTO_KEY is deliberately not listed:
it is backup key material and not this tool's to judge.
"""

# group -> environment variables that held that group's secret before the move to the store.
GROUP_ENVIRONMENT_NAMES = {
    "cloudflare": ("CLOUDFLARE_API_TOKEN", "CLOUDFLARE_TUNNEL_TOKEN"),
    "ai": ("GEMINI_API_KEY",),
    "mail": ("SMTP_PASS",),
}


# [@ANCHOR: zero_sudo:environ_secrets_names_present]
def names_present(groups, environ_path="/proc/self/environ"):
    """The sorted names, among those of `groups`, that the process environment at `environ_path` still sets
    (even to an empty value). `groups` is an iterable of group names; a group without environment names (stripe,
    hams_s3) adds none. Reads the NUL separated file once; an unreadable file raises OSError (a check that cannot
    look must not say "clean"). Returns names only."""
    wanted = {name for group in groups for name in GROUP_ENVIRONMENT_NAMES.get(group, ())}
    with open(environ_path, "rb") as handle:
        entries = handle.read().split(b"\0")
    present = {entry.split(b"=", 1)[0].decode("utf-8", "replace") for entry in entries if entry}
    return sorted(wanted & present)
