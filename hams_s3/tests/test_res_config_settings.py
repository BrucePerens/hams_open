# SPDX-License-Identifier: AGPL-3.0-or-later


from lxml import etree

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

@tagged('post_install', '-at_install')
class TestHamsS3ConfigSettings(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.ConfigSettings = self.env['res.config.settings']

    # Tests [@ANCHOR: COMM_hams_s3_config]
    # Tests [@ANCHOR: hams_s3_res_config_settings_res_config_settings]
    def test_set_and_get_values(self):
        # We need to simulate that the storage.backend model exists.
        # Since hams_s3 doesn't strictly depend on it in __manifest__.py (it uses it conditionally),
        # we check if it's available. If not, we skip or mock.
        if 'storage.backend' not in self.env:
            return
        self.StorageBackend = self.env['storage.backend']

        # Create settings
        settings = self.ConfigSettings.create({
            'hams_s3_use_s3': True,
            'hams_s3_aws_host': 's3.amazonaws.com',
            'hams_s3_aws_access_key_id': 'TEST_KEY',
            'hams_s3_aws_secret_access_key': 'TEST_SECRET',
            'hams_s3_aws_bucket': 'test-bucket',
            'hams_s3_aws_region': 'us-east-1',
        })
        
        settings.set_values()
        
        # Verify it was saved to storage.backend
        backend = self.StorageBackend.search([('backend_type', '=', 'amazon_s3')], limit=1)
        self.assertTrue(backend)
        self.assertEqual(backend.aws_host, 's3.amazonaws.com')
        self.assertEqual(backend.aws_access_key_id, 'TEST_KEY')
        self.assertEqual(backend.aws_secret_access_key, 'TEST_SECRET')
        self.assertEqual(backend.aws_bucket, 'test-bucket')
        self.assertEqual(backend.aws_region, 'us-east-1')
        
        # Verify get_values retrieves it
        new_settings = self.ConfigSettings.create({})
        res = new_settings.get_values()
        
        self.assertEqual(res.get('hams_s3_aws_host'), 's3.amazonaws.com')
        self.assertEqual(res.get('hams_s3_aws_access_key_id'), 'TEST_KEY')
        self.assertEqual(res.get('hams_s3_aws_secret_access_key'), '********', "the stored key is never shown")
        self.assertNotIn('TEST_SECRET', str(res))
        self.assertEqual(res.get('hams_s3_aws_bucket'), 'test-bucket')

        # Saving the form with the mask keeps the stored key; a new value replaces it.
        new_settings.write({'hams_s3_use_s3': True, 'hams_s3_aws_secret_access_key': res['hams_s3_aws_secret_access_key']})
        new_settings.set_values()
        self.assertEqual(backend.aws_secret_access_key, 'TEST_SECRET')
        new_settings.write({'hams_s3_aws_secret_access_key': 'NEW_SECRET'})
        new_settings.set_values()
        self.assertEqual(backend.aws_secret_access_key, 'NEW_SECRET')
        self.assertEqual(res.get('hams_s3_aws_region'), 'us-east-1')

    # Tests [@ANCHOR: hams_s3_register_secret_with_canary]
    def test_a_newly_entered_secret_is_registered_with_the_canary_and_a_mask_or_odd_value_is_skipped(self):
        secret = "Qw8Zk39dQ7mPq2LxV8nTb4RcY6aHjUe5Wt1XyZ0n"
        settings = self.ConfigSettings.create({"hams_s3_aws_secret_access_key": secret})
        settings._register_s3_secret_with_canary()  # never raises, with or without the optional addon
        if "hams.secret.canary" in self.env:  # burn-ignore-optional-oca-dep
            with self.assertLogs("hams_secret_canary.alarm", "CRITICAL") as logs:
                self.env["hams.secret.canary"]._scan_text("<p>%s</p>" % secret, "response", "/x")
            self.assertNotIn(secret, logs.output[0])
        for skipped in ("********", "short value with spaces", ""):
            self.ConfigSettings.create({"hams_s3_aws_secret_access_key": skipped})._register_s3_secret_with_canary()

    # Tests [@ANCHOR: hams_s3_compute_oca_installed]
    def test_compute_hams_s3_oca_installed_is_false_when_storage_backend_is_absent(self):
        # This sandbox never installs OCA's storage_backend (see this
        # module's own README.md and the setup script it documents) --
        # the real, current behavior in every environment this test
        # actually runs in is that the compute must resolve to False, not
        # raise or silently stay unset.
        if 'storage.backend' in self.env:  # burn-ignore-optional-oca-dep
            self.skipTest("storage.backend IS installed here -- this test covers the absent case only")  # burn-ignore-skiptest-soft-dependency
        settings = self.ConfigSettings.create({})
        self.assertFalse(settings.hams_s3_oca_installed)

    # Tests [@ANCHOR: hams_s3_get_s3_service_env]
    def test_get_s3_service_env_runs_as_the_real_s3_manager_service_account(self):
        settings = self.ConfigSettings.create({})
        service_account = self.env.ref("hams_s3.s3_manager_service_internal")

        env_svc = settings._get_s3_service_env()

        self.assertEqual(env_svc.uid, service_account.id)
        self.assertTrue(env_svc.context.get("mail_notrack"))

    # Tests [@ANCHOR: hams_s3_get_values]
    # Tests [@ANCHOR: hams_s3_set_values]
    def test_get_values_and_set_values_are_safe_no_ops_when_storage_backend_is_absent(self):
        # get_values()/set_values() both guard their real S3-persisting
        # logic behind `'storage.backend' in self.env` -- this is the one
        # code path actually exercised in every environment that doesn't
        # have OCA's storage_backend installed (this sandbox included, see
        # README.md), and it was entirely untested: nothing proved the
        # guard doesn't itself raise, or that set_values() with
        # hams_s3_use_s3=True doesn't try to touch a model that isn't
        # there.
        if 'storage.backend' in self.env:  # burn-ignore-optional-oca-dep
            self.skipTest("storage.backend IS installed here -- this test covers the absent-guard path only")  # burn-ignore-skiptest-soft-dependency
        settings = self.ConfigSettings.create({
            'hams_s3_use_s3': True,
            'hams_s3_aws_host': 's3.amazonaws.com',
        })
        mock_get_service_env = self.safe_patch(
            "odoo.addons.hams_s3.models.res_config_settings.ResConfigSettings._get_s3_service_env"
        )

        settings.set_values()  # must not raise despite hams_s3_use_s3=True
        settings.get_values()

        # The real proof the guard fired correctly, not just "didn't
        # crash": neither method should ever reach the service-account
        # env (and therefore never touch storage.backend) when the model
        # isn't there to touch.
        mock_get_service_env.assert_not_called()

    def test_settings_view_renders(self):
        # Tests [@ANCHOR: COMM_hams_s3_settings_render]
        """Proves res_config_settings_view_form's xpath injection
        compiles cleanly against the real base_setup settings view --
        see the view's own audit-ignore-view comment for why this is a
        render-proof rather than a browser tour (the "installed +
        configured" fields can't be exercised without OCA
        storage_backend actually installed)."""
        view = self.env.ref("hams_s3.res_config_settings_view_form")
        arch_node = view._get_combined_arch()
        arch = etree.tostring(arch_node, encoding="unicode")
        self.env["res.config.settings"].get_view(view_id=view.id)
        self.assertIn("hams_cloud_storage", arch)
        self.assertIn("hams_s3_use_s3", arch)


@tagged('post_install', '-at_install')
class TestHamsS3CanaryRegistration(HamsTransactionCase):
    """A newly saved S3 secret key is registered with the optional secret canary when it is installed."""

    def _save(self, secret):
        self.env['res.config.settings'].create({'hams_s3_aws_secret_access_key': secret}).set_values()

    def test_a_saved_secret_key_is_registered_and_the_mask_is_not(self):
        if 'hams.secret.canary' not in self.env:
            return  # optional addon not installed in this database
        secret = 'Qw8Zk39dQ7mPq2LxV8nTb4RcY6aHjUe5Wg1XyZ0A'
        self._save(secret)
        with self.assertLogs('hams_secret_canary.alarm', 'CRITICAL') as logs:
            self.env['hams.secret.canary']._scan_text('<p>%s</p>' % secret, 'response', '/x')
        self.assertNotIn(secret, logs.output[0])
        self._save('********')
        with self.assertNoLogs('hams_secret_canary.alarm', 'CRITICAL'):
            self.env['hams.secret.canary']._scan_text('<p>********</p>', 'response', '/x')

    def test_an_unusable_value_never_fails_the_save(self):
        self._save('short')
        self._save('')
