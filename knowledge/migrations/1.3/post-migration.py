# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Classify the manual articles that were installed before `audience` existed.

[@ANCHOR: knowledge:classify_existing_articles_migration]

Adding `knowledge.article.audience` gives every existing row the field default,
"member". That would be wrong for the developer documents zero_sudo's loader
installed years ago (stories, journeys, External Dependencies and the like), so
this runs straight after the column is added and sets each loader-installed
article from the same classifier the loader uses.

An article's identity comes from the `zero_sudo.doc_article_id_<module>_<name>`
rows in `zero_sudo_kv`, which the loader writes when it installs a document.
Each of those is matched to its `knowledge_docs` entry in the installed
modules' manifests:

* the entry classifies as member: audience member, published, readable,
  filed under the entry's help topic;
* the entry classifies as internal, or no installed manifest lists it any
  more: audience internal, unpublished, permission none;
* an article the loader never installed was written by hand. A published one
  stays a member manual (an administrator chose to publish it); an unpublished
  one becomes internal, which changes nothing a reader could see.

Nothing is deleted. Plain SQL, matching this platform's zero-sudo rule for
migration scripts. Running it again gives the same result. zero_sudo's loader
re-evaluates every document on the next registry load (its install version was
bumped), so this only has to be right until then."""

import logging

from odoo.modules.module import get_manifest

_logger = logging.getLogger(__name__)

KV_ID_PREFIX = "zero_sudo.doc_article_id_"
DEFAULT_HELP_TOPIC = "Other guides"


def _expected_classes(cr, classify):
    cr.execute(  # audit-ignore-sql: static SQL, no parameters
        "SELECT name FROM ir_module_module WHERE state = 'installed'"
    )
    expected = {}
    for (module_name,) in cr.fetchall():
        manifest = get_manifest(module_name) or {}
        docs = manifest.get("knowledge_docs")
        if not isinstance(docs, list):
            continue
        for doc_info in docs:
            name = doc_info.get("name", f"{module_name} Documentation")
            key = f"{KV_ID_PREFIX}{module_name}_{name.replace(' ', '_')}"
            audience = classify(doc_info.get("path"), doc_info)
            topic = str(doc_info.get("help_topic") or "").strip()
            expected[key] = (audience, topic or DEFAULT_HELP_TOPIC)
    return expected


def classify_existing_articles(cr):
    """Set audience, help topic, publication and permission on loader rows."""
    from odoo.addons.zero_sudo.models.ir_module_module import (
        classify_doc_audience,
    )

    cr.execute(  # audit-ignore-sql: static SQL, no parameters
        "SELECT to_regclass('zero_sudo_kv') IS NOT NULL"
    )
    if not cr.fetchone()[0]:
        return 0
    expected = _expected_classes(cr, classify_doc_audience)
    cr.execute(  # audit-ignore-sql: static SQL; the only parameter is this file's own constant prefix
        "SELECT key, value FROM zero_sudo_kv WHERE key LIKE %s",
        (KV_ID_PREFIX + "%",),
    )
    loader_ids = set()
    changed = 0
    for key, value in cr.fetchall():
        try:
            article_id = int(value)
        except (TypeError, ValueError):
            continue
        loader_ids.add(article_id)
        audience, topic = expected.get(key, ("internal", None))
        is_member = audience == "member"
        cr.execute(  # audit-ignore-sql: static SQL; parameters are values computed here, not user input
            "UPDATE knowledge_article SET audience = %s, help_topic = %s,"
            " is_published = %s, internal_permission = %s WHERE id = %s",
            (
                audience,
                topic if is_member else None,
                is_member,
                "read" if is_member else "none",
                article_id,
            ),
        )
        changed += cr.rowcount
    cr.execute(  # audit-ignore-sql: static SQL, no parameters
        "SELECT id FROM knowledge_article WHERE NOT is_published"
    )
    for (article_id,) in cr.fetchall():
        if article_id in loader_ids:
            continue
        cr.execute(  # audit-ignore-sql: static SQL; the only parameter is an id read just above
            "UPDATE knowledge_article SET audience = 'internal' WHERE id = %s",
            (article_id,),
        )
        changed += cr.rowcount
    return changed


def migrate(cr, version):
    if not version:
        return
    changed = classify_existing_articles(cr)
    _logger.info("knowledge: classified %s existing articles", changed)
