/** @odoo-module **/

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";

/**
 * URL Toast Notification Widget
 * * Adheres to the requirement mandating Odoo's native notification bus for transient actions.
 * Listens for success parameters in the URL, fires a toast notification,
 * and cleans the URL via history.replaceState to prevent duplicate triggers on refresh.
 */
export class UrlToastNotification extends Interaction {
    static selector = '#wrapwrap';

    start() {
        this._checkUrlForNotifications();
    }

    // [@ANCHOR: toast_notifications_logic]

    // Verified by [@ANCHOR: test_tour_toast_notifications]
    _checkUrlForNotifications() {
        const urlParams = new URLSearchParams(document.location.search);
        let message = '';
        let title = '';
        let type = 'success';

        // Map URL query parameters to specific notification messages
        if (urlParams.has('report_submitted')) {
            title = "Success";
            message = "We received your report and will review it.";
        } else if (urlParams.has('appeal_submitted')) {
            title = "Submitted";
            message = "We received your appeal and are reviewing it.";
        } else if (urlParams.has('subscribed')) {
            title = "Subscribed";
            message = "You will now receive weekly email digests for this site.";
        } else if (urlParams.has('erased')) {
            title = "Content Deleted";
            message = "We permanently erased your personal pages and blog posts from our servers.";
        }

        if (message) {
            // Trigger Odoo's native notification bus
            this.env.services.notification.add(message, {
                title: title,
                type: type,
                sticky: false,
            });

            // Clean up the URL to maintain a polished UX
            // We use replaceState so the user's back-button history is not affected
            const newUrl = document.location.protocol + "//" + document.location.host + document.location.pathname;
            window.history.replaceState({path: newUrl}, '', newUrl);
        }
    }
}
registry.category("public.interactions").add("user_websites.UrlToastNotification", UrlToastNotification);

// Moderation duty toast: sticky but shown at most once per period per
// (count, newest report id) so it does not nag on every tab/session.
export const MODERATION_TOAST_KEY = 'admin_violation_toast_seen';
export const MODERATION_TOAST_TTL_MS = 12 * 60 * 60 * 1000;

export function readSeen(storage) {
    try {
        const raw = (storage || window.localStorage).getItem(MODERATION_TOAST_KEY);
        return raw ? JSON.parse(raw) : null;
    } catch {
        return null;
    }
}

export function writeSeen(data, now, storage) {
    try {
        (storage || window.localStorage).setItem(MODERATION_TOAST_KEY, JSON.stringify({
            count: data.count, latest_id: data.latest_id || 0, ts: now,
        }));
    } catch {
        // Storage may be blocked (private window); the toast then simply repeats.
    }
}

/** Show when never seen, expired, or the queue grew (count or newest id up). */
export function shouldShowToast(seen, data, now) {
    if (!data || !(data.count > 0)) return false;
    if (!seen || typeof seen.ts !== 'number') return true;
    if (now - seen.ts >= MODERATION_TOAST_TTL_MS || now < seen.ts) return true;
    return data.count > (seen.count || 0) || (data.latest_id || 0) > (seen.latest_id || 0);
}

export class AdminViolationToast extends Interaction {
    static selector = '#wrapwrap';

    start() {
        this._checkPendingReports();
    }

    // [@ANCHOR: admin_toast_logic]

    // Verified by [@ANCHOR: test_tour_toast_notifications]

    // Verified by [@ANCHOR: test_admin_violation_toast_rpc]
    _checkPendingReports() {
        return fetch('/api/v1/user_websites/pending_reports')
            .then(response => {
                if (!response.ok) throw new Error("Network response was not ok");
                return response.json();
            })
            .then(data => {
                const now = Date.now();
                if (!shouldShowToast(readSeen(), data, now)) return;
                const url = data.url;
                const notify = this.env.services.notification;
                const buttons = [];
                if (typeof url === 'string' && url.startsWith('/')) {
                    buttons.push({
                        name: "Review reports",
                        primary: true,
                        onClick: () => { window.location.assign(url); },
                    });
                }
                buttons.push({ name: "Dismiss", onClick: () => {} });
                notify.add("There are " + data.count + " pending violation reports requiring review.", {
                    title: "Pending Moderation",
                    type: "warning",
                    sticky: true,
                    buttons: buttons,
                });
                writeSeen(data, now);
            }).catch(() => {
                // Silently ignore network errors to prevent UI disruption
            });
    }
}
registry.category("public.interactions").add("user_websites.AdminViolationToast", AdminViolationToast);
