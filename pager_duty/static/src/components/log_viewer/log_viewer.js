/** @odoo-module **/
import { Component, useState, onMounted } from "@odoo/owl";
import { registry } from "@web/core/registry";
import { useService } from "@web/core/utils/hooks";

export class LogViewer extends Component {
    setup() {
        this.orm = useService("orm"); // Updated to conform to OWL deprecation mandate
        this.state = useState({
            files: [],
            selectedFile: "",
            regexQuery: "",
            results: [],
            loading: false,
            error: null
        });

        onMounted(async () => {
            await this.fetchFiles();
        });
    }

    async fetchFiles() {
        try {
            const res = await fetch("/api/v1/pager/logs/files", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ params: {} })
            }).then(r => r.json());
            const data = res.result || {};
            this.state.files = data.files || [];
            if (this.state.files.length > 0) {
                this.state.selectedFile = this.state.files[0];
            }
        } catch (e) {
            this.state.error = "Failed to load configuration.";
        }
    }

    onKeyup(ev) {
        if (ev.key === "Enter") {
            this.executeSearch();
        }
    }

    async executeSearch() {
        if (!this.state.selectedFile || !this.state.regexQuery) {
            this.state.error = "Please select a file and enter a regex pattern.";
            return;
        }

        this.state.loading = true;
        this.state.error = null;
        this.state.results = [];

        try {
            const res = await fetch("/api/v1/pager/logs/search", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    params: {
                        file_path: this.state.selectedFile,
                        regex_query: this.state.regexQuery
                    }
                })
            }).then(r => r.json());
            const data = res.result || {};

            if (data.error) {
                this.state.error = data.error;
            } else {
                this.state.results = data.matches || [];
                if (this.state.results.length === 0) {
                    this.state.error = "0 matches found.";
                }
            }
        } catch (e) {
            this.state.error = "IPC Request Failed.";
        } finally {
            this.state.loading = false;
        }
    }
}
LogViewer.template = "pager_duty.LogViewerTemplate";
registry.category("actions").add("pager_duty.log_viewer", LogViewer);
