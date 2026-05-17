/** @odoo-module **/

const originalConsoleError = console.error;

// 1. Maintain a ledger of unresolved network requests to diagnose backend hangs
window._pendingRPCs = new Set();
const originalFetch = window.fetch;
window.fetch = async function(...args) {
    const url = typeof args[0] === 'string' ? args[0] : (args[0] ? args[0].url : 'unknown');
    window._pendingRPCs.add(url);
    try {
        return await originalFetch.apply(this, args);
    } finally {
        window._pendingRPCs.delete(url);
    }
};

const originalXHR = window.XMLHttpRequest.prototype.open;
window.XMLHttpRequest.prototype.open = function(method, url, ...rest) {
    this.addEventListener('loadend', () => window._pendingRPCs.delete(url));
    this.addEventListener('error', () => window._pendingRPCs.delete(url));
    this.addEventListener('abort', () => window._pendingRPCs.delete(url));
    window._pendingRPCs.add(url);
    return originalXHR.call(this, method, url, ...rest);
};

// 2. Condense the DOM into an "Interactable Skeleton" to protect LLM token limits
function buildInteractableSkeleton(node) {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent.trim();
    if (node.nodeType !== Node.ELEMENT_NODE) return "";

    // Mathematically prune structurally invisible trees
    if (node.classList && (node.classList.contains('d-none') || node.classList.contains('o_hidden'))) return "";

    // Identify semantic components critical for tour triggers
    let isImportant = ['BUTTON', 'INPUT', 'A', 'SELECT', 'TEXTAREA'].includes(node.tagName) ||
        node.hasAttribute('name') || node.hasAttribute('id') || node.hasAttribute('data-menu-xmlid') ||
        (node.classList && Array.from(node.classList).some(c => c.startsWith('o_tour_') || c === 'o_notification' || c === 'modal'));

    let childrenText = Array.from(node.childNodes).map(buildInteractableSkeleton).filter(Boolean).join(' ');

    if (isImportant) {
        let attrs = [];
        ['name', 'id', 'data-menu-xmlid', 'type', 'placeholder', 'value'].forEach(a => {
            if (node.hasAttribute(a)) attrs.push(`${a}="${node.getAttribute(a)}"`);
        });
        if (node.classList) {
            // Strip layout/utility classes, keep semantic identifiers
            let cls = Array.from(node.classList).filter(c => c.startsWith('o_') || c === 'btn' || c.startsWith('btn-')).join(' ');
            if (cls) attrs.push(`class="${cls}"`);
        }
        let tag = node.tagName.toLowerCase();
        let content = childrenText.length > 80 ? childrenText.substring(0, 80) + '...' : childrenText;
        return `\n<${tag} ${attrs.join(' ')}>${content}</${tag}>`;
    }
    return childrenText;
}

console.error = function (...args) {
    originalConsoleError.apply(console, args);

    const msg = args.map(a => {
        if (typeof a === 'string') return a;
        if (a && a.message) return a.message;
        return '';
    }).join(' ');

    if (!window._domDumped && (msg.includes('TIMEOUT') || msg.includes('FAILED:') || msg.includes('AssertionError'))) {
        window._domDumped = true;
        try {
            let rpcList = Array.from(window._pendingRPCs).join(', ') || 'None';
            let currentHash = document.location.hash || document.location.pathname;

            let stateHeader = `\n========== UI STATE SUMMARY ==========\nURL/Hash: ${currentHash}\nPending RPCs: ${rpcList}\n======================================\n`;
            let skeleton = buildInteractableSkeleton(document.body).replace(/\s{2,}/g, ' ');

            originalConsoleError.call(console, stateHeader + "\n========== INTERACTABLE DOM SKELETON ==========\n" + skeleton + "\n===============================================\n");
        } catch (e) {
            originalConsoleError.call(console, "Failed to dump DOM skeleton: " + e.toString());
        }
    }
};

// 3. Catch Unhandled Promise Rejections to prevent silent headless browser deadlocks
window.addEventListener('unhandledrejection', function(event) {
    let reason = event.reason ? (event.reason.stack || event.reason) : "Unknown Error";
    originalConsoleError.call(console, `\n========== UNHANDLED PROMISE REJECTION ==========\n${reason}\n=================================================\n`);
});

// 4. Detect illegal redirects to Discuss app (Odoo 19 fallback mechanism)
setInterval(() => {
    const url = document.location.pathname + document.location.hash + document.location.search;
    if (url.includes('/discuss')) {
        if (!window._discussAlarmTriggered) {
            window._discussAlarmTriggered = true;
            console.error("AssertionError: Tour illegally redirected to /odoo/discuss! This usually means the query parameter routing was malformed, hash routing was illegally used, or cids/menu_id were missing, causing Odoo to fallback to the default app.");
        }
    }
}, 1000);
