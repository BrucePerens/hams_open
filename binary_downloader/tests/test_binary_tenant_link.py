# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0-or-later license.
# See the LICENSE file in the project root for full license information.
import os
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

# create()/_compute_symlink_path()'s happy path is already covered by
# test_binary_manifest_integration.py's own test_pure_python_symlink_engine
# -- this file covers the remaining gaps: write() re-symlinking on an
# active_version_id change, unlink() cleaning up the OS-level symlink,
# action_upgrade_to_latest(), and _compute_symlink_path()'s own
# path-traversal-rejection branch.


@tagged("post_install", "-at_install", "standard")
class TestBinaryTenantLink(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.manager_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "binary_downloader.user_binary_downloader_service"
        )
        self.website = self.env["website"].search([], limit=1)
        if not self.website:
            self.website = self.env["website"].create({"name": "Test Tenant"})
        self.manifest = self.env["binary.manifest"].with_user(self.manager_svc).create(
            {
                "name": "tenant_link_test_bin",
                "url": "https://example.com/tenant_link_test_bin",
                "checksum": "fakehash",
            }
        )
        self.version1 = self.env["binary.version"].with_user(self.manager_svc).create(
            {
                "manifest_id": self.manifest.id,
                "version_number": "1.0",
                "url": "https://example.com/1.0",
                "checksum": "fake1",
            }
        )
        self.version2 = self.env["binary.version"].with_user(self.manager_svc).create(
            {
                "manifest_id": self.manifest.id,
                "version_number": "2.0",
                "url": "https://example.com/2.0",
                "checksum": "fake2",
            }
        )
        # apply_symlink() is exercised for real elsewhere -- here we only
        # care about which vals/actions trigger it, so patch it directly
        # rather than mocking os.symlink/os.makedirs/etc. at every call
        # site.
        self.mock_apply_symlink = self.safe_patch_object(
            type(self.env["binary.tenant.link"]), "apply_symlink", return_value=True
        )

    def test_write_re_applies_the_symlink_only_when_active_version_changes(self):
        # Tests [@ANCHOR: binary_tenant_link_write]
        link = self.env["binary.tenant.link"].with_user(self.manager_svc).create(
            {
                "website_id": self.website.id,
                "manifest_id": self.manifest.id,
                "active_version_id": self.version1.id,
            }
        )
        self.mock_apply_symlink.reset_mock()

        link.with_user(self.manager_svc).write({"name": "renamed, not a version change"})
        self.mock_apply_symlink.assert_not_called()

        link.with_user(self.manager_svc).write({"active_version_id": self.version2.id})
        self.mock_apply_symlink.assert_called_once()

    def test_unlink_removes_the_real_os_level_symlink(self):
        # Tests [@ANCHOR: binary_tenant_link_unlink]
        link = self.env["binary.tenant.link"].with_user(self.manager_svc).create(
            {
                "website_id": self.website.id,
                "manifest_id": self.manifest.id,
                "active_version_id": self.version1.id,
            }
        )
        symlink_path = link.symlink_path
        tenant_dir = os.path.dirname(symlink_path)
        os.makedirs(tenant_dir, exist_ok=True)
        os.symlink("/nonexistent/fake/central/path", symlink_path)
        self.assertTrue(os.path.lexists(symlink_path))

        link.with_user(self.manager_svc).unlink()
        self.assertFalse(
            os.path.lexists(symlink_path),
            "[!] DIAGNOSTIC FOR AI: unlink() must remove the real OS-level symlink, not just the DB row.",
        )

    def test_action_upgrade_to_latest_repoints_to_the_newest_version(self):
        # Tests [@ANCHOR: binary_tenant_link_action_upgrade_to_latest]
        link = self.env["binary.tenant.link"].with_user(self.manager_svc).create(
            {
                "website_id": self.website.id,
                "manifest_id": self.manifest.id,
                "active_version_id": self.version1.id,
            }
        )
        result = link.with_user(self.manager_svc).action_upgrade_to_latest()
        self.assertEqual(link.active_version_id, self.version2)
        self.assertEqual(result.get("type"), "ir.actions.client")

    def test_action_upgrade_to_latest_is_a_no_op_when_already_current(self):
        # Tests [@ANCHOR: binary_tenant_link_action_upgrade_to_latest]
        link = self.env["binary.tenant.link"].with_user(self.manager_svc).create(
            {
                "website_id": self.website.id,
                "manifest_id": self.manifest.id,
                "active_version_id": self.version2.id,
            }
        )
        result = link.with_user(self.manager_svc).action_upgrade_to_latest()
        self.assertEqual(link.active_version_id, self.version2)
        self.assertEqual(result.get("params", {}).get("title"), "Up to Date")

    def test_compute_symlink_path_rejects_a_manifest_name_with_a_slash(self):
        # Tests [@ANCHOR: binary_tenant_link_compute_symlink_path]
        # binary.manifest._check_name_no_slashes() already blocks a
        # slash-containing name at the manifest level for any real,
        # saved record -- this branch is unreachable through the
        # ordinary ORM create()/write() path. Exercised via new()
        # in-memory records (which never run @api.constrains) to prove
        # _compute_symlink_path() has its own, independent
        # defense-in-depth check rather than relying solely on the
        # manifest's own constraint.
        unsaved_manifest = self.env["binary.manifest"].new(
            {"name": "not/a/valid/name"}
        )
        link = self.env["binary.tenant.link"].new(
            {
                "website_id": self.website.id,
                "manifest_id": unsaved_manifest,
            }
        )
        link._compute_symlink_path()
        self.assertFalse(link.symlink_path)
