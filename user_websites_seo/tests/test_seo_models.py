# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import AccessError


from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestSEOModels(RealTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.user_admin = cls.env.ref("base.user_admin")

        cls.regular_user1 = cls.env["res.users"].create(
            {
                "name": "Regular User 1",
                "login": "reg1",
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )

        cls.regular_user2 = cls.env["res.users"].create(
            {
                "name": "Regular User 2",
                "login": "reg2",
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )

        cls.group = cls.env["user.websites.group"].create(
            {
                "name": "Test SEO Group",
                "website_slug": "test-seo-group",
                "member_ids": [(6, 0, [cls.regular_user1.id])],
            }
        )

    def test_self_writeable_fields(self):
        # Tests [@ANCHOR: COMM_res_users_self_writeable_fields]

        # [@ANCHOR: COMM_test_self_writeable_fields]
        """Test that SEO fields are added to writeable fields for users."""
        fields = self.env["res.users"].SELF_WRITEABLE_FIELDS
        seo_fields = [
            "website_meta_title",
            "website_meta_description",
            "website_meta_keywords",
            "website_meta_og_img",
            "seo_name",
        ]
        for f in seo_fields:
            self.assertIn(f, fields)

        # The list alone proves nothing about whether Odoo's actual
        # self-write bypass works for these fields -- prove a non-admin
        # user can really write one of them on their own record.
        self.regular_user1.with_user(self.regular_user1).write(
            {"seo_name": "reg1-updated-seo-name"}
        )
        self.assertEqual(self.regular_user1.seo_name, "reg1-updated-seo-name")

    def test_check_access_rule_res_users(self):
        # Tests [@ANCHOR: COMM_res_users_seo_write_elevation]

        # [@ANCHOR: COMM_test_check_access_rule_res_users]
        """Test that a user can write to their own SEO fields but not others."""
        # reg1 can write to their own profile
        reg1_record = self.regular_user1.with_user(self.regular_user1)
        # Should not raise exception
        reg1_record.write({"website_meta_title": "My Title"})

        # reg1 cannot write to reg2
        reg2_record_by_reg1 = self.regular_user2.with_user(self.regular_user1)
        msg = "A user MUST NOT be able to modify the SEO of another user."
        self.env.flush_all()
        with self.assertRaises(AccessError, msg=msg):
            reg2_record_by_reg1.write({"website_meta_title": "Hacked Title"})
            self.env.flush_all()
        self.env.flush_all()

    def test_check_access_rule_res_users_seo_self_write_survives_a_non_main_company(self):
        # Tests [@ANCHOR: COMM_res_users_seo_write_elevation]

        # night_shift_todo/medium/seo-mixin-res-users-multi-company-elevation-4765a849.md
        # theorized (from reading the code, not from running it) that a
        # portal user outside the SEO service account's company_ids
        # (user_websites/data/user_websites_data.xml: [base.main_company]
        # only) would be wrongly denied writing their own SEO fields, since
        # SEOMetadataMixin.write()'s self.with_user(svc_uid) elevation is
        # subject to Odoo core's global res_users_rule
        # (['|', ('share','=',False), ('company_ids','in',company_ids)]).
        #
        # Built a real second res.company and a portal user scoped to it
        # to check that theory against real behavior, not just the code.
        # It does not reproduce: res.users.write() (odoo/addons/base/
        # models/res_users.py) has its OWN, earlier self-write bypass --
        # when self == self.env.user and every vals key is in
        # SELF_WRITEABLE_FIELDS (which ResUsersSEO extends with exactly
        # the SEO fields), it does `self = self.sudo()` before ever
        # calling further into the write chain. A self-write of an SEO
        # field satisfies both conditions, so it is sudo'd -- and, since
        # self.env.su is then True, SEOMetadataMixin.write()'s own
        # `if self.env.su: return super().write(vals)` fast path returns
        # immediately, never reaching the svc_uid elevation branch or its
        # company check at all. Confirmed directly (not assumed): calling
        # the elevation branch's own with_user(svc_uid)/write() sequence
        # in isolation, bypassing the self-write fast path via
        # skip_seo_metadata_mixin, DOES raise AccessError for this same
        # non-main-company record -- the theoretical gap in the elevation
        # branch itself is real, it is just unreachable from res.users'
        # own self-write path, which is the only path _check_seo_write_
        # permission() ever allows to succeed (it is self-only). Whether
        # any OTHER model using this mixin's elevation branch (e.g.
        # user.websites.group, which has no equivalent native self-write
        # bypass of its own) can actually reach that same company check on
        # a genuine self-write was not checked here and is a narrower,
        # separate question than what this to-do asked.
        other_company = self.env["res.company"].create({"name": "Non-Main Test Company"})
        other_company_user = self.env["res.users"].create(
            {
                "name": "Other Company Portal User",
                "login": "other_company_portal_user",
                "company_id": other_company.id,
                "company_ids": [(6, 0, [other_company.id])],
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.env.flush_all()
        other_company_user.with_user(other_company_user).write(
            {"website_meta_title": "My Title From A Non-Main Company"}
        )
        self.assertEqual(
            other_company_user.website_meta_title,
            "My Title From A Non-Main Company",
            "A portal user outside the SEO service account's company_ids must still be able to "
            "write their own SEO fields -- the self-write bypass in res.users.write() covers this "
            "before SEOMetadataMixin's own elevation branch (and its company check) is ever reached.",
        )

        # The elevation branch's own company check is still a real,
        # separate latent gap -- confirmed directly here rather than
        # inferred, by exercising it in isolation (skip_seo_metadata_mixin
        # skips straight past the self-write fast path that normally
        # shields this branch from ever running for res.users).
        utils = self.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("user_websites.user_websites_service_account")
        with self.assertRaises(
            AccessError,
            msg=(
                "Known, narrower gap than this to-do described: the SEO mixin's OWN svc_uid "
                "elevation branch is still subject to Odoo's global company-scoped res_users_rule, "
                "and svc_uid's company_ids is [base.main_company] only. Unreachable for res.users' "
                "own self-write (see this test's own docstring), but real for any other model using "
                "this mixin without an equivalent native self-write bypass."
            ),
        ):
            other_company_user.with_user(svc_uid).with_context(skip_seo_metadata_mixin=True).write(
                {"website_meta_title": "Elevation branch, isolated"}
            )

    def test_check_access_rule_user_websites_group(self):
        # Tests [@ANCHOR: COMM_user_websites_group_seo_write_elevation]

        # [@ANCHOR: COMM_test_check_access_rule_user_websites_group]
        """Test that a user can write to a group they are a member of."""
        # reg1 is a member, can write
        group_by_reg1 = self.group.with_user(self.regular_user1)
        # Should not raise exception
        group_by_reg1.write({"website_meta_title": "Group Title"})

        # reg2 is not a member, cannot write
        group_by_reg2 = self.group.with_user(self.regular_user2)
        msg = "A user MUST NOT be able to modify SEO of not owned group."
        self.env.flush_all()
        with self.assertRaises(AccessError, msg=msg):
            group_by_reg2.write({"website_meta_title": "Hacked Group Title"})
            self.env.flush_all()
        self.env.flush_all()

    def test_check_access_rule_user_websites_group_seo_self_write_denied_for_a_non_main_company(self):
        # Tests [@ANCHOR: COMM_user_websites_group_seo_write_elevation]

        # night_shift_todo/medium/seo-mixin-group-company-elevation-b12c2366.md: the narrower,
        # not-yet-confirmed sibling of
        # test_check_access_rule_res_users_seo_self_write_survives_a_non_main_company above.
        # That test found res.users' own self-write is SHIELDED from
        # SEOMetadataMixin's svc_uid-elevation company check by an earlier,
        # unrelated bypass in res.users.write() itself (SELF_WRITEABLE_FIELDS
        # -> self.sudo()). user.websites.group has no such bypass (confirmed
        # by reading write() in user_websites/models/user_websites_groups.py:
        # it wraps super().write() in a savepoint for slug-constraint error
        # relabeling only, nothing self/sudo related), so a genuine member's
        # self-write of their own group's SEO fields should reach the
        # mixin's elevation branch for real, and from there the same global
        # res_users_rule company check confirmed real (in isolation) by the
        # test above -- svc_uid's company_ids is [base.main_company] only.
        # Built a real second res.company and a group actually scoped to it
        # (company_id is a required Many2one, default self.env.company, so
        # must be passed explicitly at create time) to check this for real,
        # not by re-applying the res.users result: confirmed to reproduce.
        other_company = self.env["res.company"].create({"name": "Non-Main Test Company 2"})
        other_company_member = self.env["res.users"].create(
            {
                "name": "Other Company Group Member",
                "login": "other_company_group_member",
                "company_id": other_company.id,
                "company_ids": [(6, 0, [other_company.id])],
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        other_company_group = self.env["user.websites.group"].create(
            {
                "name": "Other Company Test SEO Group",
                "website_slug": "other-company-test-seo-group",
                "company_id": other_company.id,
                "member_ids": [(6, 0, [other_company_member.id])],
            }
        )
        self.env.flush_all()
        with self.assertRaises(
            AccessError,
            msg=(
                "This is the real, non-latent gap seo-mixin-group-company-elevation-b12c2366.md "
                "asked to confirm: a user.websites.group member, self-writing their own group's "
                "SEO fields, has no native self-write bypass shielding them from "
                "SEOMetadataMixin's svc_uid elevation, and svc_uid's company_ids "
                "([base.main_company] only) does not cover this group's own non-main company."
            ),
        ):
            other_company_group.with_user(other_company_member).write(
                {"website_meta_title": "Group Title From A Non-Main Company"}
            )
            self.env.flush_all()
        self.env.flush_all()

    def test_xpath_rendering_res_users(self):
        # [@ANCHOR: COMM_test_xpath_rendering_res_users]
        """Prove that the SEO notebook page correctly renders in res.users."""
        res = self.env["res.users"].get_view(
            view_id=self.env.ref("base.view_users_form").id, view_type="form"
        )
        self.assertIn(
            'name="user_websites_seo_settings"',
            res["arch"],
            "The SEO notebook page must exist in res.users arch.",
        )
        self.assertIn(
            'name="website_meta_og_img"',
            res["arch"],
            "The Social Media field must exist in res.users arch.",
        )

    def test_xpath_rendering_user_websites_group(self):
        # [@ANCHOR: COMM_test_xpath_rendering_user_websites_group]
        """Prove that the SEO notebook page renders in user.websites.group."""
        res = self.env["user.websites.group"].get_view(
            view_id=self.env.ref("user_websites.view_user_websites_group_form").id,
            view_type="form",
        )
        self.assertIn(
            'name="group_seo_settings"',
            res["arch"],
            "The SEO notebook page must exist in user.websites.group arch.",
        )
        self.assertIn(
            'name="website_meta_og_img"',
            res["arch"],
            "The Social Media field must exist in user.websites.group arch.",
        )

    def test_xpath_rendering_pages_posts(self):
        # [@ANCHOR: COMM_test_xpath_rendering_pages_posts]
        """Prove that the SEO page renders in website.page and blog.post."""
        # Test website.page
        res_page = self.env["website.page"].get_view(
            view_id=self.env.ref("website.website_pages_form_view").id, view_type="form"
        )
        arch = res_page["arch"]
        msg = "The SEO page must exist in website.page arch."
        self.assertIn('name="seo_settings"', arch, msg)

        # Test blog.post
        res_post = self.env["blog.post"].get_view(
            view_id=self.env.ref("website_blog.view_blog_post_form").id,
            view_type="form",
        )
        arch = res_post["arch"]
        msg = "The SEO page must exist in blog.post arch."
        self.assertIn('name="seo_settings"', arch, msg)

    def test_mixin_base_check_seo_write_permission_is_abstract(self):
        # Tests [@ANCHOR: user_websites_seo:COMM_mixin_check_seo_write_permission]
        """Every concrete model using this mixin overrides
        _check_seo_write_permission() with its own real check -- the base
        implementation itself is never reached in normal operation, so
        nothing else in this test suite ever actually calls it. Prove it
        directly: it must refuse to silently allow anything, raising
        NotImplementedError rather than defaulting to permissive."""
        mixin = self.env["user.websites.seo.metadata.mixin"]
        with self.assertRaises(NotImplementedError):
            mixin._check_seo_write_permission()
