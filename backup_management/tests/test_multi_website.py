# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase
from odoo.exceptions import AccessError

@tagged("post_install", "-at_install", "multi_website")
class TestBackupMultiWebsite(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.Website = self.env['website']
        self.BackupConfig = self.env['backup.config']

        # Create two websites
        self.website_1 = self.Website.create({'name': 'Website 1'})
        self.website_2 = self.Website.create({'name': 'Website 2'})

        # Create a user for each website
        # Note: In Odoo 19, users have a website_id which is used for record rules if configured
        self.user_1 = self.env['res.users'].create({
            'name': 'User 1',
            'login': 'user1',
            'email': 'user1@test.com',
            'website_id': self.website_1.id,
            'group_ids': [(6, 0, [self.env.ref('backup_management.group_backup_admin').id])]
        })
        self.user_2 = self.env['res.users'].create({
            'name': 'User 2',
            'login': 'user2',
            'email': 'user2@test.com',
            'website_id': self.website_2.id,
            'group_ids': [(6, 0, [self.env.ref('backup_management.group_backup_admin').id])]
        })

        # Create configurations for each website
        self.config_1 = self.BackupConfig.create({
            'name': 'Config 1',
            'engine': 'kopia',
            'target_path': '/var/lib/odoo/backups/repo1',
            'website_id': self.website_1.id
        })
        self.config_2 = self.BackupConfig.create({
            'name': 'Config 2',
            'engine': 'kopia',
            'target_path': '/var/lib/odoo/backups/repo2',
            'website_id': self.website_2.id
        })
        self.config_global = self.BackupConfig.create({
            'name': 'Config Global',
            'engine': 'kopia',
            'target_path': '/var/lib/odoo/backups/global',
            'website_id': False
        })

    def test_record_rules_isolation(self):
        # User 1 should see Config 1 and Config Global, but not Config 2
        configs_user1 = self.BackupConfig.with_user(self.user_1).search([])
        self.assertIn(self.config_1, configs_user1)
        self.assertIn(self.config_global, configs_user1)
        self.assertNotIn(self.config_2, configs_user1)

        # User 2 should see Config 2 and Config Global, but not Config 1
        configs_user2 = self.BackupConfig.with_user(self.user_2).search([])
        self.assertIn(self.config_2, configs_user2)
        self.assertIn(self.config_global, configs_user2)
        self.assertNotIn(self.config_1, configs_user2)

    def test_job_propagation(self):
        # Triggering a backup from Config 1 should create a job with website_1
        self.config_1.with_user(self.user_1).action_trigger_backup()
        job = self.env['backup.job'].search([('config_id', '=', self.config_1.id)], limit=1)
        self.assertEqual(job.website_id, self.website_1)

        # User 2 should not see User 1's jobs
        jobs_user2 = self.env['backup.job'].with_user(self.user_2).search([])
        self.assertNotIn(job, jobs_user2)

    def test_snapshot_isolation(self):
        # Snapshot for Config 1
        snap = self.env['backup.snapshot'].create({
            'config_id': self.config_1.id,
            'snapshot_id': 'snap1',
        })

        # User 1 should see it
        snaps_user1 = self.env['backup.snapshot'].with_user(self.user_1).search([])
        self.assertIn(snap, snaps_user1)

        # User 2 should NOT see it
        snaps_user2 = self.env['backup.snapshot'].with_user(self.user_2).search([])
        self.assertNotIn(snap, snaps_user2)
