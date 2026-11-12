/** @odoo-module **/
import { Component, useState, onMounted, onWillUnmount } from "@odoo/owl";
import { registry } from "@web/core/registry";
import { useService } from "@web/core/utils/hooks";

// [@ANCHOR: pager_duty_js_board_pager_board]
export class PagerBoard extends Component {
    // [@ANCHOR: pager_duty:js_board_setup]
    setup() {
        this.busService = useService("bus_service");
        this.orm = useService("orm");
        this.state = useState({
            active_incidents: [],
            resolved_incidents: [],
            on_duty: "Loading...",
            stats: { passing: 0, failing: 0, maintenance: 0, never_run: 0 },
            transformStyle: "",
            isPaused: false
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

    // [@ANCHOR: pager_duty:js_board_fetch_data]
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
        this.state.stats = data.stats;
        this.applyBurnInShift();
    }

    // [@ANCHOR: pager_duty:js_board_apply_burn_in_shift]
    applyBurnInShift() {
        // Pseudo-random drift between -15px and +15px to prevent OLED burn-in
        const x = Math.floor(Math.random() * 30) - 15;
        const y = Math.floor(Math.random() * 30) - 15;
        // 20-second linear transition makes the movement visually imperceptible
        this.state.transformStyle = `transform: translate(${x}px, ${y}px); transition: transform 20s linear;`;
    }

    // [@ANCHOR: pager_duty:js_board_acknowledge]
    async acknowledge(incidentId) {
        await this.orm.call("pager.incident", "action_acknowledge", [[incidentId]]);
        await this.fetchData();
    }

    // [@ANCHOR: pager_duty:js_board_toggle_pause]
    togglePause() {
        this.state.isPaused = !this.state.isPaused;
    }

    // [@ANCHOR: pager_duty:js_board_on_notification]
    onNotification({ detail: notifications }) {
        if (this.state.isPaused) return;
        for (const { type } of notifications) {
            if (type === "update_board") {
                this.fetchData().catch((err) => console.error("[PagerBoard] fetchData failed:", err));
            }
        }
    }
}
PagerBoard.template = "pager_duty.PagerBoardTemplate";
registry.category("actions").add("pager_duty.board", PagerBoard);
