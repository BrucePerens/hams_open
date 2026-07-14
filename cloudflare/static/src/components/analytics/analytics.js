/* Copyright © HAMS project. AGPL-3.0. */
/** @odoo-module **/

import { Component, onWillStart, useState, onMounted, onWillUnmount } from "@odoo/owl";
import { registry } from "@web/core/registry";

export class CloudflareAnalyticsDashboard extends Component {
    setup() {
        this.state = useState({
            requests: 0,
            bandwidth: '0 GB',
            threats: 0,
            loading: true,
            driftX: 0,
            driftY: 0,
        });

        onWillStart(async () => {
            // Mock data fetching for now
            await new Promise((resolve) => setTimeout(resolve, 500));
            this.state.requests = 154200;
            this.state.bandwidth = '45.2 GB';
            this.state.threats = 1205;
            this.state.loading = false;
        });

        onMounted(() => {
            this.driftInterval = setInterval(() => {
                this.state.driftX = Math.floor(Math.random() * 4) - 2;
                this.state.driftY = Math.floor(Math.random() * 4) - 2;
            }, 20000);
        });

        onWillUnmount(() => {
            clearInterval(this.driftInterval);
        });
    }
}

CloudflareAnalyticsDashboard.template = "cloudflare.AnalyticsDashboard";

registry.category("actions").add("cloudflare_analytics_dashboard", CloudflareAnalyticsDashboard);
