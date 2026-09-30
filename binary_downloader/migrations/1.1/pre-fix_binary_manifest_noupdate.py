# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""`data/binary_manifest_data.xml`'s three `binary.manifest` records (kopia, etcd, cloudflared)
were originally declared inside a `<data noupdate="1">` block. Found live 2026-09-30 while bumping
etcd, root-caused directly against Odoo 19's own source (see hams_com's own night_shift_history.md
2026-09-30 entry for the full mechanism): `odoo/tools/convert.py`'s `XmlImport._tag_record()`
checks the XML importer's own per-`<data>`-block PARSE-TIME `noupdate` flag BEFORE `_load_records()`
is ever called, so a record inside `noupdate="1"` silently never picks up a source-file edit on any
`-u` upgrade to an already-installed database.

That file's `noupdate="1"` wrapper has already been removed in this same change (unlike
`ham_init/migrations/0.5`/`0.6`'s target, `website.default_website`, these three rows are owned by
`binary_downloader` itself, not a foreign module -- so there is no permanently-foreign noupdate flag
to work around, only this module's own already-installed rows, whose `ir_model_data.noupdate` was
set `True` at first install and has stayed `True` ever since). But removing `noupdate="1")` from the
XML file only changes what a *fresh* install does; it does nothing for a row that already exists
with `noupdate=True` in an already-installed database -- `_load_records()`'s own separate DB-level
gate (`not (update and d_noupdate)`) would still block the write.

This is a PRE-migration (runs before this module's own data files are (re)loaded in the same `-u`
pass, confirmed directly from `odoo/modules/migration.py`'s own docstring: "pre-" scripts run
before module initialisation, "post-" after) that flips these three existing rows' own
`ir_model_data.noupdate` to `False` via plain SQL (this platform's zero-sudo rule against ever
constructing a SUPERUSER_ID environment in a migration script). Once that's done, the SAME `-u` run's
own subsequent standard XML-data-loading path picks up and applies whatever
`data/binary_manifest_data.xml` currently says -- today's etcd v3.7.2 bump, and every future bump to
this file -- without needing a hand-written SQL re-apply of each individual field (unlike the
`website.default_website` case, which had to copy the target field's value directly because that
record belongs to a different module entirely)."""

_TARGET_XMLIDS = (
    "binary_manifest_kopia",
    "binary_manifest_etcd",
    "binary_manifest_cloudflared",
)


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, no interpolated values -- module/name are this migration's own hardcoded xmlids
        "UPDATE ir_model_data SET noupdate = false "
        "WHERE module = 'binary_downloader' AND name = ANY(%s)",
        (list(_TARGET_XMLIDS),),
    )
