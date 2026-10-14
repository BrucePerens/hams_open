# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Regression for helpdesk ticket 70: creating a blog.post from the back
office (backend JSON-RPC, no `request.website`) raised AttributeError out of
the Cloudflare purge hook. Tests [@ANCHOR: user_websites:COMM_blog_post_invalidate_cloudflare_cache]
"""
import json
import types
import uuid

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestBlogPostWithoutWebsiteRequest(HamsHttpCase):
    def setUp(self):
        super().setUp()
        uid = uuid.uuid4().hex[:8]
        self.website = self.env["website"].get_current_website()
        self.owner = self.env["res.users"].create(
            {
                "name": f"Bulletin Owner {uid}",
                "login": f"bulletin_{uid}",
                "email": f"bulletin_{uid}@example.com",
                "website_slug": f"bulletin_{uid}",
                "group_ids": [
                    (6, 0, [
                        self.env.ref("base.group_portal").id,
                        self.env.ref("user_websites.group_user_websites_user").id,
                    ])
                ],
            }
        )
        self.blog = self.env["blog.blog"].create(
            {"name": f"Blog {uid}", "website_id": self.website.id, "owner_user_id": self.owner.id}
        )
        self.uid_ = uid

    def _vals(self, suffix):
        return {
            "name": f"Post {self.uid_} {suffix}",
            "blog_id": self.blog.id,
            "owner_user_id": self.owner.id,
        }

    def _fake_backend_request(self):
        fake = types.SimpleNamespace()  # deliberately no `.website`
        fake._get_current_object = lambda: fake
        self.safe_patch("odoo.addons.cloudflare.models.edge_context.request", new=fake)

    def test_create_with_request_lacking_website_attribute(self):
        self._fake_backend_request()
        post = self.env["blog.post"].create(self._vals("orm"))
        self.assertTrue(post.exists())
        queued = self.env["cloudflare.purge.queue"].search(
            [("target_item", "=", f"site-{self.owner.website_slug}")]
        )
        self.assertTrue(queued, "purge tag should still be queued")

    def test_backend_jsonrpc_web_save_as_admin(self):
        self.authenticate("admin", "admin")
        resp = self.url_open(
            "/web/dataset/call_kw/blog.post/web_save",
            data=json.dumps(
                {
                    "jsonrpc": "2.0",
                    "method": "call",
                    "params": {
                        "model": "blog.post",
                        "method": "web_save",
                        "args": [[], self._vals("rpc")],
                        "kwargs": {"specification": {"name": {}}},
                    },
                }
            ),
            headers={"Content-Type": "application/json"},
        )
        body = resp.json()
        self.assertNotIn("error", body, body.get("error"))
        self.assertEqual(body["result"][0]["name"], f"Post {self.uid_} rpc")
        self.assertTrue(self.env["blog.post"].search([("name", "=", f"Post {self.uid_} rpc")]))

    def test_enqueue_failure_never_fails_create_or_write(self):
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.CloudflarePurgeQueue.enqueue_tags",
            side_effect=AttributeError("boom"),
        )
        with self.assertLogs("odoo.addons.user_websites.models.blog_post", level="WARNING"):
            post = self.env["blog.post"].create(self._vals("fail"))
        self.assertTrue(post.exists())
        with self.assertLogs("odoo.addons.user_websites.models.blog_post", level="WARNING"):
            post.write({"name": post.name + " edited"})
        self.assertTrue(post.name.endswith("edited"))
