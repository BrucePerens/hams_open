/** @odoo-module **/
/* Copyright © Bruce Perens K6BP. All Rights Reserved.
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */
import { Component, useState, onMounted, onWillUnmount } from "@odoo/owl";
import { registry } from "@web/core/registry";
import { useService } from "@web/core/utils/hooks";

export class BackupBoard extends Component {
    setup() {
        this.orm = useService("orm");
        this.website = useService("website");
        this.state = useState({
            configs: [],
            isLoading: true,
        });

        onMounted(async () => {
            await this.fetchData();
        });

        onWillUnmount(() => {
        });
    }

    async triggerBackup(configId) {
        await this.orm.call("backup.config", "action_trigger_backup", [configId]);
        await this.fetchData();
    }

    async syncSnapshots(configId) {
        await this.orm.call("backup.config", "action_sync_snapshots", [configId]);
        await this.fetchData();
    }

    async fetchData() {
        // Isolation by website_id
        const context = {};
        if (this.website.currentWebsite) {
            context.website_id = this.website.currentWebsite.id;
        }
        this.state.configs = await this.orm.call("backup.config", "get_board_data", [], { context: context });
        this.state.isLoading = false;
    }
}
BackupBoard.template = "backup_management.BackupBoardTemplate";
registry.category("actions").add("backup_management.board", BackupBoard);
