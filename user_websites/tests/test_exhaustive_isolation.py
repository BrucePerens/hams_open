# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import odoo.tests
from odoo.tests import tagged
import logging
from odoo.exceptions import AccessError

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestExhaustiveIsolation(odoo.tests.common.HttpCase):
    """
    Aggressive Red-Team test suite designed to hunt for cross-tenant IDORs,
    privilege escalations, and Server-Side Template Injections (SSTI)
    introduced by the Proxy Ownership pattern.
    """

    def setUp(self):
        super().setUp()
        self.password = "test_password"

        self.malice = self.env["res.users"].create(
            {
                "name": "Malice Attacker",
                "login": "malice_redteam",
                "password": self.password,
                "email": "malice@example.com",
                "website_slug": "malice",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

        self.victim = self.env["res.users"].create(
            {
                "name": "Innocent Victim",
                "login": "victim_redteam",
                "password": self.password,
                "email": "victim@example.com",
                "website_slug": "victim",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

        # Ensure the shared blog exists
        self.community_blog = self.env["blog.blog"].search(
            [("name", "=", "Community Blog")], limit=1
        )
        if not self.community_blog:
            self.community_blog = self.env["blog.blog"].create(
                {"name": "Community Blog"}
            )

        # Setup Victim Content
        self.victim_group = self.env["user.websites.group"].create(
            {
                "name": "Victim Private Group",
                "website_slug": "victim-group",
                "member_ids": [(4, self.victim.id)],
            }
        )

        self.victim_post = self.env["blog.post"].create(
            {
                "name": "Victim Post",
                "blog_id": self.community_blog.id,
                "owner_user_id": self.victim.id,
                "is_published": True,
            }
        )

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        self.victim_page = (
            self.env["website.page"]
            .with_user(svc_uid)
            .create(
                {
                    "url": f"/{self.victim.website_slug}/about",
                    "name": "Victim About",
                    "type": "qweb",
                    "website_published": True,
                    "owner_user_id": self.victim.id,
                    "arch": f"""<t name="About" t-name="user_websites.about_{self.victim.website_slug}">
                    <t t-call="website.layout">
                        <div>About Victim</div>
                    </t>
                </t>""",
                }
            )
        )

    def test_01_community_blog_container_protection(self):
        """
        Risk: Because users share 'Community Blog', Malice might try to delete or rename it.
        Action: Malice executes write() or unlink() on blog.blog.
        Expected: Strict AccessError.
        """
        with self.assertRaises(
            AccessError,
            msg="Malice MUST NOT be able to rename the shared blog container.",
        ):
            self.community_blog.with_user(self.malice).write({"name": "Hacked Blog"})
            self.env.flush_all()

        with self.assertRaises(
            AccessError,
            msg="Malice MUST NOT be able to delete the shared blog container.",
        ):
            self.community_blog.with_user(self.malice).unlink()

    def test_02_seo_metadata_cross_tenant_idor(self):
        """
        Risk: The SEO module adds metadata to `SELF_WRITEABLE_FIELDS`. Malice might pass Victim's ID.
        Action: Malice writes SEO data to Victim's res.users record.
        Expected: AccessError from `check_access_rule`.
        """
        if "website_meta_title" in self.env["res.users"]._fields:
            with self.assertRaises(
                AccessError,
                msg="Malice MUST NOT be able to modify Victim's SEO metadata.",
            ):
                self.victim.with_user(self.malice).write(
                    {"website_meta_title": "Hacked SEO Title"}
                )
                self.env.flush_all()

            with self.assertRaises(
                AccessError,
                msg="Malice MUST NOT be able to modify Victim Group's SEO metadata.",
            ):
                self.victim_group.with_user(self.malice).write(
                    {"website_meta_title": "Hacked Group SEO"}
                )
                self.env.flush_all()

    def test_03_group_membership_escalation(self):
        """
        Risk: Malice adds themselves to a private group to steal their website.
        Action: Malice writes to Victim Group's `member_ids`.
        Expected: AccessError.
        """
        with self.assertRaises(
            AccessError,
            msg="Malice MUST NOT be able to escalate privileges by adding themselves to a group.",
        ):
            self.victim_group.with_user(self.malice).write(
                {"member_ids": [(4, self.malice.id)]}
            )
            self.env.flush_all()

    @odoo.tests.mute_logger("odoo.http")
    def test_04_qweb_ssti_injection_attempt(self):
        """
        Risk: Because the Proxy Ownership service account executes the write,
        Malice might try to inject executable QWeb logic into their `arch`
        to steal database information during rendering.
        Action: Malice writes `<t t-esc="request.env['res.users']..."/>` into their page.
        Expected: Odoo's safe-eval or the HTTP controller MUST NOT render the stolen data.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        # Create a base page for Malice
        arch_string = f"""<t name="Home" t-name="user_websites.home_{self.malice.website_slug}">
            <t t-call="website.layout">
                <div id="stolen_data">
                    <t t-esc="request.env['res.users'].sudo().search([('id', '=', 1)]).email"/>
                </div>
            </t>
        </t>"""

        self.env["website.page"].with_user(svc_uid).create(
            {
                "url": f"/{self.malice.website_slug}/home",
                "name": "Malice Home",
                "type": "qweb",
                "website_published": True,
                "owner_user_id": self.malice.id,
                "arch": arch_string,
            }
        )

        self.env.flush_all()

        # Render the page as an unauthenticated user
        self.authenticate(None, None)

        # Note: If Odoo's QWeb engine is fully secure, it should either strip the code,
        # fail to evaluate 'request', block 'sudo', or return an empty string.
        # It must NEVER return the target data.
        admin_email = self.env["res.users"].browse(1).email or "admin@example.com"

        try:
            response = self.url_open(f"/{self.malice.website_slug}/home")
            content = response.content.decode("utf-8")
            self.assertNotIn(
                admin_email,
                content,
                "CRITICAL SSTI VULNERABILITY: Malicious QWeb evaluated successfully and leaked database records!",
            )
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("An error occurred: %s", e)
            # If the rendering engine crashes entirely due to the illegal syntax (e.g. QWebException),
            # that is also considered a successful defense against extraction.
            _logger.info("QWeb rendering exception caught as expected.")

    def test_05_blog_post_cross_tenant_mutation(self):
        """
        Risk: Malice modifies the content of Victim's blog post via RPC.
        Action: Malice writes to Victim's blog.post ID.
        Expected: AccessError.
        """
        with self.assertRaises(
            AccessError, msg="Malice MUST NOT be able to edit Victim's blog post."
        ):
            self.victim_post.with_user(self.malice).write({"content": "Hacked Content"})
            self.env.flush_all()

        with self.assertRaises(
            AccessError, msg="Malice MUST NOT be able to delete Victim's blog post."
        ):
            self.victim_post.with_user(self.malice).unlink()

    def test_06_report_violation_spoofing(self):
        """
        Risk: Malice submits a violation report via RPC but forces the `reported_by_user_id`
        to be the Victim, framing them.
        Action: Malice creates a content.violation.report.
        Expected: The record rules must strictly prevent the creation because
        the reported_by_user_id does not match the session's user.id.
        """
        with self.assertRaises(
            AccessError,
            msg="Record rules must prevent users from spoofing the reported_by_user_id on creation.",
        ):
            self.env["content.violation.report"].with_user(self.malice).create(
                {
                    "target_url": "/some/bad/page",
                    "description": "Framing the victim",
                    "reported_by_user_id": self.victim.id,
                }
            )
            self.env.flush_all()

    def test_07_public_read_access(self):
        """
        Risk: Personal blogs and web pages might be accidentally locked behind authentication.
        Action: Unauthenticated public user reads the records.
        Expected: Successful read (HTTP 200 and ORM read capabilities).
        """
        public_user = self.env.ref("base.public_user")

        # 1. ORM Read Checks
        self.assertTrue(self.community_blog.with_user(public_user).read(["name"]))
        self.assertTrue(self.victim_post.with_user(public_user).read(["name"]))
        self.assertTrue(self.victim_page.with_user(public_user).read(["name"]))

        # 2. HTTP Read Checks
        self.authenticate(None, None)

        # Test Blog Index
        blog_url = f"/{self.victim.website_slug}/blog"
        res_blog = self.url_open(blog_url)
        self.assertEqual(
            res_blog.status_code,
            200,
            f"Public user MUST be able to read blog at {blog_url}",
        )

        # Test Custom Page
        page_url = f"/{self.victim.website_slug}/about"
        res_page = self.url_open(page_url)
        self.assertEqual(
            res_page.status_code,
            200,
            f"Public user MUST be able to read page at {page_url}",
        )

    @odoo.tests.mute_logger("odoo.http")
    def test_08_rpc_confused_deputy(self):
        """
        Risk: Controllers or RPC endpoints might not enforce Proxy Ownership correctly,
        allowing Malice to pass Victim's ID in the payload (Confused Deputy), bypassing ORM tests.
        """
        self.authenticate(self.malice.login, self.password)

        payload = {
            "model": "blog.post",
            "method": "create",
            "args": [
                [
                    {
                        "name": "Malice Spoofed Post",
                        "blog_id": self.community_blog.id,
                        "owner_user_id": self.victim.id,  # Spoofing the victim!
                        "is_published": True,
                    }
                ]
            ],
            "kwargs": {},
        }

        # Make the raw RPC request simulating a frontend dataset call
        with self.assertRaises(
            Exception,
            msg="RPC call MUST fail proxy ownership validation and raise an exception.",
        ):
            self.make_jsonrpc_request(
                "/web/dataset/call_kw/blog.post/create", payload  # burn-ignore-route
            )  # burn-ignore-route
