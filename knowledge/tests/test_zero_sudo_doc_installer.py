# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
# Tests of zero_sudo's knowledge_docs installer (ir_module_module.py) that need the real
# `knowledge.article` model. They live here, not in zero_sudo/tests/, because zero_sudo does
# not (and must not) depend on `knowledge`; a bare `-u zero_sudo` never installs it.
from odoo.modules.module import get_manifest
from odoo.tests.common import tagged
from odoo.tools import file_open

from odoo.addons.zero_sudo.tests import common


@tagged("post_install", "-at_install")
class TestZeroSudoDocInstaller(common.HamsTransactionCase):
    def test_bootstrap_knowledge_docs_default_unpublished(self):
        # Tests [@ANCHOR: zero_sudo:install_single_doc]
        # Regression test: module-authored knowledge_docs (architecture notes,
        # security internals, runbooks, developer/story/journey guides) must
        # default to is_published=False. A prior bug forced every ingested doc
        # to is_published=True unconditionally, leaking internal engineering
        # documentation (e.g. "Zero-Sudo Security Core") into the public
        # website's anonymous help sidebar. See night_shift_todo.md.
        #
        # zero_sudo's own __manifest__.py deliberately does NOT depend on
        # `knowledge` (`_bootstrap_knowledge_docs` gracefully no-ops when it isn't
        # installed), so these tests live in the module that owns the dependency:
        # knowledge depends on zero_sudo, so they are always run with both installed.
        Module = self.env["ir.module.module"]
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]

        doc_info = {
            "name": "Test Internal Doc Regression",
            "path": "data/documentation.html",
        }
        Module._install_single_doc(utils, Article, "zero_sudo", doc_info)
        article = Article.search([("name", "=", "Test Internal Doc Regression")], limit=1)
        self.assertTrue(article, "Doc should have been created")
        self.assertFalse(
            article.is_published,
            "Module-authored docs must default to unpublished (internal-only) "
            "unless the manifest entry explicitly sets \"public\": True",
        )
        self.assertEqual(
            article.internal_permission,
            "none",
            "Internal documentation must be admin-only, not readable by every internal user.",
        )

    def test_bootstrap_knowledge_docs_explicit_public_opt_in(self):
        Module = self.env["ir.module.module"]
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]

        doc_info = {
            "name": "Test Public Doc Regression",
            "path": "data/documentation.html",
            "public": True,
        }
        Module._install_single_doc(utils, Article, "zero_sudo", doc_info)
        article = Article.search([("name", "=", "Test Public Doc Regression")], limit=1)
        self.assertTrue(article, "Doc should have been created")
        self.assertTrue(
            article.is_published,
            "A knowledge_docs entry with \"public\": True should be published",
        )
        self.assertEqual(article.internal_permission, "read")

    def test_story_journey_and_runbook_docs_are_never_public(self):
        # Tests [@ANCHOR: zero_sudo:install_single_doc]
        # Stories, journeys and runbooks are internal engineering documents.
        # Even a manifest entry that says "public": True must not publish one.
        Module = self.env["ir.module.module"]
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]

        doc_info = {
            "name": "Test Story Marked Public",
            "path": "docs/stories/search.md",
            "public": True,
        }
        with self.assertLogs("odoo.addons.zero_sudo.models.ir_module_module", "ERROR"):
            Module._install_single_doc(utils, Article, "knowledge", doc_info)
        article = Article.search([("name", "=", "Test Story Marked Public")], limit=1)
        self.assertTrue(article)
        self.assertFalse(article.is_published)
        self.assertEqual(article.internal_permission, "none")

    def test_no_manifest_publishes_a_story_journey_or_runbook(self):
        # Tests [@ANCHOR: zero_sudo:install_single_doc]
        # Covers every installed module in this database, so a manifest that
        # tries to publish an internal document fails the suite of the
        # repository that ships it.
        Module = self.env["ir.module.module"]
        offenders = []
        installed = Module.search([("state", "=", "installed")])
        for module in installed:
            manifest = get_manifest(module.name)
            for doc_info in (manifest or {}).get("knowledge_docs", []):
                if doc_info.get("public") and Module._is_internal_doc_path(doc_info.get("path", "")):
                    offenders.append(f"{module.name}: {doc_info['path']}")
        self.assertEqual(offenders, [])

    def test_markdown_docs_are_stored_as_html(self):
        # Tests [@ANCHOR: zero_sudo:install_single_doc]
        # A markdown file used to be stored as raw markdown text, which the
        # viewer then had to guess at. Literal tag text in the file, such as
        # a heading written as `<h2>`, defeated the guess and the article
        # was served as raw markdown.
        Module = self.env["ir.module.module"]
        html = Module._markdown_doc_to_html(
            "# Title\n\nUse the `<h2>` tag.\n\n- one\n- two\n\n<script>window.bad=1</script>\n"
        )
        self.assertIn("<h1", html)
        self.assertIn("<li>one</li>", html)
        self.assertIn("&lt;h2&gt;", html)
        self.assertNotIn("<script", html)

        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]
        Module._install_single_doc(
            utils, Article, "knowledge", {"name": "Test Markdown Doc", "path": "docs/stories/search.md"}
        )
        article = Article.search([("name", "=", "Test Markdown Doc")], limit=1)
        self.assertTrue(article)
        self.assertRegex(article.body, r"<h[1-6][ >]")
        self.assertNotRegex(article.body, r"(?m)^#{1,6}\s")

    def test_install_single_doc_does_not_collide_across_modules_sharing_a_name(self):
        # Tests [@ANCHOR: zero_sudo:install_single_doc]
        # Regression test for a real 2026-09-13 bug-hunt finding: the
        # "does an article for this doc already exist" lookup used to be
        # `Article.search([("name", "=", name)], limit=1)` (or the
        # equivalent `article_by_name` bulk map) -- scoped ONLY by the
        # doc's display `name`, with no module component at all, while the
        # change-detection hash key right next to it was correctly scoped
        # by (module_name, name). Two different modules declaring a
        # `knowledge_docs` entry with the SAME name (plausible: generic
        # titles like "Getting Started" are exactly what two unrelated
        # modules might independently pick) would collide: whichever
        # module was processed SECOND in a batch would silently overwrite
        # the FIRST module's article. Fixed by tracking each module's own
        # article id in a `zero_sudo.kv` entry keyed the same way the
        # content hash already is, and looking existence up by that id
        # (re-checked via .exists(), never by searching `name`).
        Module = self.env["ir.module.module"]
        utils = self.env["zero_sudo.security.utils"]
        Article = self.env["knowledge.article"]

        shared_name = "Collision Test Doc"
        # Two REAL modules that each really ship a file at this same
        # relative path, with genuinely different content -- exercises the
        # actual manifest-driven code path (get_module_path/file_open),
        # not a synthetic stand-in.
        doc_info_a = {"name": shared_name, "path": "data/documentation.html"}
        doc_info_b = {"name": shared_name, "path": "data/documentation.html"}
        module_a = "zero_sudo"
        module_b = "knowledge"

        hash_key_a = f"zero_sudo.doc_hash_{module_a}_{shared_name.replace(' ', '_')}"
        hash_key_b = f"zero_sudo.doc_hash_{module_b}_{shared_name.replace(' ', '_')}"
        article_id_key_a = f"zero_sudo.doc_article_id_{module_a}_{shared_name.replace(' ', '_')}"
        article_id_key_b = f"zero_sudo.doc_article_id_{module_b}_{shared_name.replace(' ', '_')}"

        # Simulate one real bulk batch: both entries share one pre-loaded
        # (empty) existing_hashes/existing_article_ids snapshot, exactly as
        # _bootstrap_knowledge_docs builds once up front for the whole
        # batch before dispatching per-doc.
        empty_hashes = {}
        empty_article_ids = {}
        Module._install_single_doc(
            utils, Article, module_a, doc_info_a, empty_hashes, empty_article_ids
        )
        Module._install_single_doc(
            utils, Article, module_b, doc_info_b, empty_hashes, empty_article_ids
        )

        # Both modules must end up with their OWN distinct article -- not
        # one overwriting the other.
        matching = Article.search([("name", "=", shared_name)])
        self.assertEqual(
            len(matching), 2,
            "Two modules sharing a knowledge_docs name must each get their "
            "own knowledge.article, not collide onto a single record.",
        )

        article_id_a = int(utils._get_kv(article_id_key_a))
        article_id_b = int(utils._get_kv(article_id_key_b))
        self.assertNotEqual(
            article_id_a, article_id_b,
            "Each module's own article-identity KV entry must point at a "
            "DIFFERENT article record.",
        )
        article_a = Article.browse(article_id_a)
        article_b = Article.browse(article_id_b)
        # `body` is an Html(sanitize=True) field -- Odoo's sanitizer can
        # reformat markup on write, so compare on a distinguishing text
        # snippet from each real source file rather than exact string
        # equality against the raw file content.
        marker_a = "Zero-Sudo Security: User Guide"
        marker_b = "Welcome to Manuals"
        with file_open(f"{module_a}/data/documentation.html", "r") as f:
            self.assertIn(
                marker_a, f.read(),
                "Sanity check on the test fixture itself.",
            )
        with file_open(f"{module_b}/data/documentation.html", "r") as f:
            self.assertIn(
                marker_b, f.read(),
                "Sanity check on the test fixture itself.",
            )
        self.assertIn(
            marker_a, article_a.body,
            "zero_sudo's own article must carry zero_sudo's own content.",
        )
        self.assertNotIn(
            marker_b, article_a.body,
            "zero_sudo's own article must NOT have been overwritten by "
            "knowledge's content.",
        )
        self.assertIn(
            marker_b, article_b.body,
            "knowledge's own article must carry knowledge's own content.",
        )
        self.assertNotIn(
            marker_a, article_b.body,
            "knowledge's own article must NOT have been overwritten by "
            "zero_sudo's content.",
        )

        write_date_a_before = article_a.write_date
        write_date_b_before = article_b.write_date

        # Re-run with UNCHANGED content, this time with the real,
        # just-written KV state bulk-loaded (mirroring how
        # _bootstrap_knowledge_docs pre-loads both dicts up front for a
        # real batch) -- must be a true no-op: no new article created, and
        # neither existing article rewritten, via the article-id-based
        # existence check finding the same two records again.
        real_hashes = {
            hash_key_a: utils._get_kv(hash_key_a),
            hash_key_b: utils._get_kv(hash_key_b),
        }
        real_article_ids = {
            article_id_key_a: utils._get_kv(article_id_key_a),
            article_id_key_b: utils._get_kv(article_id_key_b),
        }
        Module._install_single_doc(
            utils, Article, module_a, doc_info_a, real_hashes, real_article_ids
        )
        Module._install_single_doc(
            utils, Article, module_b, doc_info_b, real_hashes, real_article_ids
        )

        matching_after = Article.search([("name", "=", shared_name)])
        self.assertEqual(
            len(matching_after), 2,
            "Re-running with unchanged content must not create duplicate "
            "articles.",
        )
        self.assertEqual(
            article_a.write_date, write_date_a_before,
            "Re-running with unchanged content must not rewrite the "
            "existing article (content hash unchanged should short-circuit "
            "before ever reaching the write/create branch).",
        )
        self.assertEqual(
            article_b.write_date, write_date_b_before,
            "Re-running with unchanged content must not rewrite the "
            "existing article (content hash unchanged should short-circuit "
            "before ever reaching the write/create branch).",
        )
