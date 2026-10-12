# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Classifier, loader and migration for the article audience."""
import importlib.util
import os
from unittest.mock import patch

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.models.ir_module_module import (
    classify_doc_audience,
)
from odoo.addons.zero_sudo.tests import common

MIGRATION_PATH = os.path.join(
    os.path.dirname(__file__),
    "..",
    "migrations",
    "1.3",
    "post-migration.py",
)


def _load_migration():
    spec = importlib.util.spec_from_file_location(
        "knowledge_post_migration_1_3", MIGRATION_PATH
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install")
class TestClassifyDocAudience(common.HamsTransactionCase):
    def test_01_unknown_is_internal(self):
        # Tests [@ANCHOR: zero_sudo:classify_doc_audience]
        self.assertEqual(
            classify_doc_audience("data/documentation.html", {"name": "X"}),
            "internal",
        )
        self.assertEqual(classify_doc_audience("", {}), "internal")
        self.assertEqual(classify_doc_audience(None, None), "internal")

    def test_02_explicit_opt_in_is_member(self):
        path = "data/documentation.html"
        self.assertEqual(
            classify_doc_audience(path, {"name": "X", "public": True}),
            "member",
        )
        self.assertEqual(
            classify_doc_audience(path, {"name": "X", "audience": "member"}),
            "member",
        )
        self.assertEqual(
            classify_doc_audience(path, {"name": "X", "public": "yes"}),
            "internal",
        )
        self.assertEqual(
            classify_doc_audience(path, {"audience": "internal", "public": True}),
            "internal",
        )

    def test_03_developer_markers_beat_the_opt_in(self):
        flagged = {"public": True, "audience": "member"}
        for path in (
            "docs/stories/x.md",
            "docs/journeys/x.md",
            "docs/runbooks/x.md",
            "Docs/Stories/x.md",
            "README.md",
            "docs/readme.html",
        ):
            self.assertEqual(
                classify_doc_audience(path, dict(flagged, name="A Guide")),
                "internal",
                path,
            )
        for title in ("Story: Searching", "journey: Admin", "  Story: x"):
            self.assertEqual(
                classify_doc_audience(
                    "data/documentation.html", dict(flagged, name=title)
                ),
                "internal",
                title,
            )

    def test_04_every_shipped_member_doc_names_a_help_topic(self):
        from odoo.modules.module import get_manifest

        modules = self.env["ir.module.module"].search([("state", "=", "installed")])
        for module in modules:
            for doc in (get_manifest(module.name) or {}).get("knowledge_docs", []):
                if classify_doc_audience(doc["path"], doc) == "member":
                    self.assertTrue(
                        doc.get("help_topic"),
                        f"{module.name}: {doc['name']} has no help_topic",
                    )


@tagged("post_install", "-at_install")
class TestInstallerSetsAudience(common.HamsTransactionCase):
    def _install(self, doc_info):
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]
        self.env["ir.module.module"]._install_single_doc(
            utils, Article, "knowledge", doc_info
        )
        return Article.search([("name", "=", doc_info["name"])], limit=1)

    def test_00_model_door_matches_the_function(self):
        # Tests [@ANCHOR: zero_sudo:doc_audience_for]
        Module = self.env["ir.module.module"]
        info = {"name": "X", "public": True}
        self.assertEqual(
            Module._doc_audience_for("data/documentation.html", info), "member"
        )
        self.assertEqual(
            Module._doc_audience_for("docs/stories/a.md", info), "internal"
        )

    def test_01_member_doc_is_published_with_topic(self):
        article = self._install(
            {
                "name": "Aud Test Member Guide",
                "path": "data/documentation.html",
                "public": True,
                "help_topic": "Community",
            }
        )
        self.assertEqual(article.audience, "member")
        self.assertTrue(article.is_published)
        self.assertEqual(article.internal_permission, "read")
        self.assertEqual(article.help_topic, "Community")

    def test_02_member_doc_without_topic_gets_the_default(self):
        article = self._install(
            {
                "name": "Aud Test No Topic",
                "path": "data/documentation.html",
                "public": True,
            }
        )
        self.assertEqual(article.help_topic, "Other guides")

    def test_03_unflagged_doc_is_internal_and_hidden(self):
        article = self._install(
            {"name": "Aud Test Unflagged", "path": "data/documentation.html"}
        )
        self.assertEqual(article.audience, "internal")
        self.assertFalse(article.is_published)
        self.assertEqual(article.internal_permission, "none")
        self.assertFalse(article.help_topic)

    def test_04_story_cannot_be_exposed_by_a_public_flag(self):
        article = self._install(
            {
                "name": "Aud Test Story",
                "path": "docs/stories/search.md",
                "public": True,
                "help_topic": "Community",
            }
        )
        self.assertEqual(article.audience, "internal")
        self.assertFalse(article.is_published)


@tagged("post_install", "-at_install")
class TestClassifyExistingArticlesMigration(common.HamsTransactionCase):
    def _kv(self, module, name, article):
        key = "zero_sudo.doc_article_id_%s_%s" % (module, name.replace(" ", "_"))
        self.env["zero_sudo.security.utils"]._set_kv(key, str(article.id))

    def test_01_classifies_loader_articles_and_keeps_hand_written_ones(self):
        # Tests [@ANCHOR: knowledge:classify_existing_articles_migration]
        Article = self.env["knowledge.article"]
        legacy = {
            "is_published": True,
            "internal_permission": "read",
            "audience": "member",
        }
        guide = Article.create(dict(legacy, name="Mig Member Guide"))
        story = Article.create(dict(legacy, name="Story: Mig Thing"))
        stale = Article.create(dict(legacy, name="Mig Removed From Manifest"))
        hand_published = Article.create(dict(legacy, name="Mig Hand Published"))
        hand_draft = Article.create(
            {"name": "Mig Hand Draft", "is_published": False}
        )
        self._kv("knowledge", "Mig Member Guide", guide)
        self._kv("knowledge", "Story: Mig Thing", story)
        self._kv("knowledge", "Mig Removed From Manifest", stale)
        manifest = {
            "knowledge_docs": [
                {
                    "name": "Mig Member Guide",
                    "path": "data/documentation.html",
                    "public": True,
                    "help_topic": "Community",
                },
                {
                    "name": "Story: Mig Thing",
                    "path": "docs/stories/x.md",
                    "public": True,
                },
            ]
        }
        migration = _load_migration()
        with patch.object(migration, "get_manifest", return_value=manifest):
            migration.classify_existing_articles(self.env.cr)
            first = (guide + story + stale + hand_published + hand_draft)
            first.invalidate_recordset()
            snapshot = [
                (a.audience, a.is_published, a.internal_permission, a.help_topic)
                for a in first
            ]
            migration.classify_existing_articles(self.env.cr)
            first.invalidate_recordset()
            again = [
                (a.audience, a.is_published, a.internal_permission, a.help_topic)
                for a in first
            ]
        self.assertEqual(snapshot, again, "the migration is idempotent")
        self.assertEqual(snapshot[0], ("member", True, "read", "Community"))
        self.assertEqual(snapshot[1], ("internal", False, "none", False))
        self.assertEqual(snapshot[2], ("internal", False, "none", False))
        self.assertEqual(snapshot[3][:2], ("member", True))
        self.assertEqual(snapshot[4][:2], ("internal", False))
        self.assertTrue(
            Article.search_count([("id", "in", first.ids)]) == 5,
            "nothing is deleted",
        )
