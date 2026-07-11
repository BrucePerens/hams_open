/** @odoo-module **/

import { Component, onWillStart, useState } from "@odoo/owl";
import { registry } from "@web/core/registry";

export class CloudflareAnalyticsDashboard extends Component {
    setup() {
        this.state = useState({
            requests: 0,
            bandwidth: '0 GB',
            threats: 0,
            loading: true,
        });

        onWillStart(async () => {
            // Mock data fetching for now
            await new Promise((resolve) => setTimeout(resolve, 500));
            this.state.requests = 154200;
            this.state.bandwidth = '45.2 GB';
            this.state.threats = 1205;
            this.state.loading = false;
        });
    }
}

CloudflareAnalyticsDashboard.template = "cloudflare.AnalyticsDashboard";

registry.category("actions").add("cloudflare_analytics_dashboard", CloudflareAnalyticsDashboard);
