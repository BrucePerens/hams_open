/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { registry } from "@web/core/registry";
import { useService } from "@web/core/utils/hooks";
import { Component, onWillStart, useState, useRef, useEffect } from "@odoo/owl";

export class SecurityDashboard extends Component {
    setup() {
        this.orm = useService("orm");
        this.action = useService("action");
        this.state = useState({
            logs: [],
            isPaused: false,
        });
        this.dashboardRoot = useRef("dashboardRoot");

        useEffect(() => {
            const interval = setInterval(() => {
                if (this.dashboardRoot.el && !this.state.isPaused) {
                    const x = Math.floor(Math.random() * 6) - 3;
                    const y = Math.floor(Math.random() * 6) - 3;
                    this.dashboardRoot.el.style.transform = `translate(${x}px, ${y}px)`;
                }
            }, 20000);
            return () => clearInterval(interval);
        }, []);

        onWillStart(async () => {
            await this.loadLogs();
        });
    }

    async loadLogs() {
        this.state.logs = await this.orm.searchRead(
            "zero_sudo.security.log",
            [],
            ["id", "create_date", "user_id", "login", "ip_address", "reason"],
            { limit: 100, order: "create_date desc" }
        );
    }

    togglePause() {
        this.state.isPaused = !this.state.isPaused;
    }
}

SecurityDashboard.template = "zero_sudo.SecurityDashboard";

registry.category("actions").add("zero_sudo.security_dashboard", SecurityDashboard);
