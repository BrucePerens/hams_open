# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.tests.common import TransactionCase
from odoo.tests import tagged

@tagged('post_install', '-at_install')
class TestHamsS3ConfigSettings(TransactionCase):

    def setUp(self):
        super().setUp()
        self.ConfigSettings = self.env['res.config.settings']

    # [@ANCHOR: COMM_COMM_hams_s3_config]
    # Tests [@ANCHOR: COMM_hams_s3_config]
    def test_set_and_get_values(self):
        # We need to simulate that the storage.backend model exists.
        # Since hams_s3 doesn't strictly depend on it in __manifest__.py (it uses it conditionally),
        # we check if it's available. If not, we skip or mock.
        if 'storage.backend' not in self.env:
            return

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
        self.assertEqual(res.get('hams_s3_aws_secret_access_key'), 'TEST_SECRET')
        self.assertEqual(res.get('hams_s3_aws_bucket'), 'test-bucket')
        self.assertEqual(res.get('hams_s3_aws_region'), 'us-east-1')
