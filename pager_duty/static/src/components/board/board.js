/** @odoo-module **/
import { Component, useState, onMounted, onWillUnmount } from "@odoo/owl";
import { registry } from "@web/core/registry";
import { useService } from "@web/core/utils/hooks";

export class PagerBoard extends Component {
    setup() {
        this.busService = useService("bus_service");
        this.orm = useService("orm");
        this.state = useState({
            active_incidents: [],
            resolved_incidents: [],
            on_duty: "Loading...",
            transformStyle: ""
        });

        onMounted(async () => {
            this.busService.addChannel("pager_duty");
            this.busService.addEventListener("notification", this.onNotification.bind(this));
            await this.fetchData();
            this.burnInTimer = setInterval(() => this.applyBurnInShift(), 60000);
        });

        onWillUnmount(() => {
            if (this.burnInTimer) clearInterval(this.burnInTimer);
        });
    }

    async fetchData() {
        const context = {};
        const urlParams = new URLSearchParams(document.location.search);
        if (urlParams.has('website_id')) {
            context.website_id = parseInt(urlParams.get('website_id'));
        }
        const data = await this.orm.call("pager.incident", "get_board_data", [], { context });
        this.state.active_incidents = data.active;
        this.state.resolved_incidents = data.resolved;
        this.state.on_duty = data.on_duty;
        this.applyBurnInShift();
    }

    applyBurnInShift() {
        // Pseudo-random drift between -15px and +15px to prevent OLED burn-in
        const x = Math.floor(Math.random() * 30) - 15;
        const y = Math.floor(Math.random() * 30) - 15;
        // 20-second linear transition makes the movement visually imperceptible
        this.state.transformStyle = `transform: translate(${x}px, ${y}px); transition: transform 20s linear;`;
    }

    async acknowledge(incidentId) {
        await this.orm.call("pager.incident", "action_acknowledge", [[incidentId]]);
        await this.fetchData();
    }

    onNotification({ detail: notifications }) {
        for (const { type } of notifications) {
            if (type === "update_board") {
                this.fetchData();
            }
        }
    }
}
PagerBoard.template = "pager_duty.PagerBoardTemplate";
registry.category("actions").add("pager_duty.board", PagerBoard);
