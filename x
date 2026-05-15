README.md:* **[True Environment Parity](test_real_transaction/README.md) (`test_real_transaction`):** A testing facility that bypasses Odoo's test cursor wrapper for true database commits and cross-worker behavior testing.
backup_management/tests/test_backup.py:from odoo.addons.test_real_transaction.tests.real_transaction import RealTransactionCase
docs/adrs/MASTER_12_QA_TESTING_MANDATES.md:* **Anti-Mocking:** The `test_real_transaction.RealTransactionCase` facility MUST be used in favor of mocking to solve cross-thread or cross-transaction isolation issues (e.g., HTTP Controllers, Redis Pub/Sub, external daemons). It completely bypasses Odoo's test cursor wrapping, allowing tests to perform actual `env.cr.commit()` operations.
docs/modules/test_real_transaction.md:# 🧪 Real Transaction Testing Facility (`test_real_transaction`)
docs/modules/test_real_transaction.md:from odoo.addons.test_real_transaction.tests.real_transaction import RealTransactionCase
docs/modules/test_real_transaction.md:**CRITICAL:** Do not assume the class is in a file named `test_real_transaction.py`.
docs/modules/test_real_transaction.md:* [Real Transaction Testing](test_real_transaction/docs/stories/real_transaction_testing.md)
docs/modules/test_real_transaction.md:* [Documentation Injection](test_real_transaction/docs/stories/documentation_injection.md)
docs/modules/test_real_transaction.md:* [Developer Testing Flow](test_real_transaction/docs/journeys/developer_testing_flow.md)
docs/modules/test_real_transaction.md:* [Documentation Setup Flow](test_real_transaction/docs/journeys/documentation_setup_flow.md)
prompts/review_test_real_transaction.md:# Task: Deep Review of the `test_real_transaction` Module
prompts/review_test_real_transaction.md:You are tasked with performing a comprehensive, deep review of the `test_real_transaction` module in this Odoo repository.
prompts/review_test_real_transaction.md:Begin your deep planning mode to understand the `test_real_transaction` module's purpose. Explore its files, assess what major features might be missing for its target audience, create a plan, and execute it autonomously.
user_websites/tests/test_cache_coherence.py:from odoo.addons.test_real_transaction.tests.real_transaction import RealTransactionCase
user_websites/tests/test_controllers.py:from odoo.addons.test_real_transaction.tests.real_transaction import RealTransactionCase
hams_test/static/src/js/tours/test_real_transaction_tour.js:registry.category("web_tour.tours").add("test_real_transaction_tour", {
hams_test/static/src/js/tours/test_real_transaction_tour.js:            trigger: 'a.o_menu_entry_lvl_2[data-menu-xmlid="test_real_transaction.menu_noisy_table"]',
hams_test/views/noisy_table_views.xml:        <field name="name">test_real_transaction.noisy_table.list</field>
hams_test/views/noisy_table_views.xml:        <field name="model">test_real_transaction.noisy_table</field>
hams_test/views/noisy_table_views.xml:        <field name="name">test_real_transaction.noisy_table.form</field>
hams_test/views/noisy_table_views.xml:        <field name="model">test_real_transaction.noisy_table</field>
hams_test/views/noisy_table_views.xml:        <field name="res_model">test_real_transaction.noisy_table</field>
hams_test/tests/test_ui.py:        self.start_tour("/web", 'test_real_transaction_tour', login="admin")
hams_test/tests/test_facility.py:from odoo.addons.test_real_transaction.tests.real_transaction import RealTransactionCase
hams_test/tests/test_facility.py:        noisy_tables_records = self.env['test_real_transaction.noisy_table'].search([])
hams_test/tests/test_facility.py:        noisy_table_record = self.env['test_real_transaction.noisy_table'].create({
hams_test/tests/test_facility.py:        noisy_tables_records = self.env['test_real_transaction.noisy_table'].search([])
hams_test/tests/real_transaction.py:        if "test_real_transaction.noisy_table" in self.env:
hams_test/tests/real_transaction.py:            noisy_records = self.env["test_real_transaction.noisy_table"].search(
hams_test/security/ir.model.access.csv:access_test_real_transaction_noisy_table,test_real_transaction.noisy_table,model_test_real_transaction_noisy_table,group_real_transaction_manager,1,1,1,1
hams_test/models/noisy_table.py:    _name = 'test_real_transaction.noisy_table'
hams_test/docs/journeys/documentation_setup_flow.md:This journey describes how the `test_real_transaction` module ensures its documentation is properly installed.
hams_test/docs/journeys/documentation_setup_flow.md:3.  **Bootstrap Trigger**: Once the registry is ready, the `test_real_transaction.noisy_table` model executes its `_register_hook` ([@ANCHOR: documentation_bootstrap]).
hams_test/docs/journeys/documentation_setup_flow.md:7.  **Content Loading**: The documentation content is read from `test_real_transaction/data/documentation.html`.
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_bus_bus" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_ir_logging" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_base_registry_signaling" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_ir_cron" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_mail_message" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_mail_notification" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_mail_followers" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_mail_tracking_value" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_res_groups_users_rel" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_res_company_users_rel" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_res_users_log" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_http_session" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_database_pg_setting" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_database_table_stat" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_database_query_stat" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_database_activity" model="test_real_transaction.noisy_table">
hams_test/data/noisy_table_data.xml:        <record id="noisy_table_database_index_stat" model="test_real_transaction.noisy_table">
hams_test/data/documentation.html:<h1>Real Transaction Testing Facility (<code>test_real_transaction</code>)</h1>
hams_test/data/documentation.html:<p>The <code>test_real_transaction</code> module solves this by providing a drop-in replacement: <strong><code>RealTransactionCase</code></strong>.
hams_test/data/documentation.html:from odoo.addons.test_real_transaction.tests.real_transaction import RealTransactionCase
