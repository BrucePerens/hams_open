# SPDX-License-Identifier: AGPL-3.0-or-later
"""Group `hams_s3` of hams_module_secrets_from_store: the S3 secret key is served from the secret store and
never typed into, shown by or kept in the Settings form. Test values only. Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets")."""

from odoo.exceptions import UserError
from odoo.tests import tagged

from odoo.addons.hams_s3.models import res_config_settings as rcs
from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.models import module_secrets as ms
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

STORE_SECRET = "test-s3-store-secret-0000"
TYPED = "test-s3-typed-secret-1111"


class _DictBackend:
    label = "test"

    def __init__(self, values):
        self.values = values

    def get(self, name):
        return self.values.get(ss.check_name(name))


@tagged("post_install", "-at_install")
class TestHamsS3SecretStoreGroup(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.store_values = {rcs.STORE_NAME: STORE_SECRET}
        self.safe_patch_object(
            ms, "module_secret_store", side_effect=lambda: ss.SecretStore([_DictBackend(self.store_values)]))

    def _group(self, on):
        self.safe_patch_object(ms, "enabled_groups", return_value=frozenset({"hams_s3"}) if on else frozenset())

    def _stored_typed_rows(self):
        self.env.flush_all()
        self.env.cr.execute("SELECT count(*) FROM res_config_settings WHERE hams_s3_aws_secret_access_key = %s", [TYPED])
        return self.env.cr.fetchone()[0]

    # Tests [@ANCHOR: hams_s3_refuse_typed_secret]
    # Tests [@ANCHOR: hams_s3_settings_create]
    # Tests [@ANCHOR: hams_s3_settings_write]
    def test_group_on_refuses_a_typed_secret_and_stores_nothing(self):
        self._group(True)
        Settings = self.env["res.config.settings"]
        with self.assertRaises(UserError):
            Settings.create({"hams_s3_aws_secret_access_key": TYPED})
        settings = Settings.create({"hams_s3_aws_secret_access_key": rcs.SECRET_MASK})
        with self.assertRaises(UserError):
            settings.write({"hams_s3_aws_secret_access_key": TYPED})
        self.assertEqual(self._stored_typed_rows(), 0)

    # Tests [@ANCHOR: hams_s3_get_values]
    def test_group_on_shows_only_the_mask_and_group_off_is_unchanged(self):
        self._group(True)
        res = self.env["res.config.settings"].get_values()
        self.assertEqual(res["hams_s3_aws_secret_access_key"], rcs.SECRET_MASK)
        self.assertNotIn(STORE_SECRET, str(res))
        self.store_values.clear()
        self.assertFalse(self.env["res.config.settings"].get_values()["hams_s3_aws_secret_access_key"])
        self._group(False)
        self.assertNotEqual(self.env["res.config.settings"].get_values().get("hams_s3_aws_secret_access_key"), rcs.SECRET_MASK)
        self.env["res.config.settings"].create({"hams_s3_aws_secret_access_key": TYPED})
        self.assertEqual(self._stored_typed_rows(), 1)
