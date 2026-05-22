/** @odoo-module **/

const originalConsoleError = console.error;
const originalLog = console.log;
const originalWarn = console.warn;
const originalInfo = console.info;

// Messages that trigger browser warning/error severity but should NOT fail the Odoo Python test suite.
const BENIGN_MESSAGES = [
    "Owl is running in 'dev' mode",
    "ResizeObserver loop limit exceeded",
    "ResizeObserver loop completed with undelivered notifications"
];

function isBenign(args) {
    const msg = args.map(a => typeof a === 'string' ? a : (a && a.message ? a.message : '')).join(' ');
    return BENIGN_MESSAGES.some(benignMsg => msg.includes(benignMsg));
}

// Downgrade benign messages to standard logs. This whitelists them, preserving visibility
// in the console and test runner, while bypassing Odoo's native Python failure traps.
console.warn = function(...args) {
    if (isBenign(args)) {
        originalLog.apply(console, ["[WHITELISTED WARN]"].concat(args));
        return;
    }
    originalWarn.apply(console, args);
};

console.info = function(...args) {
    if (isBenign(args)) {
        originalLog.apply(console, ["[WHITELISTED INFO]"].concat(args));
        return;
    }
    originalInfo.apply(console, args);
};

// =================================================================================
// 1. INSTANT ABORT RELAY TRIGGER
// =================================================================================
function triggerInstantAbort(reason, details) {
    if (window._hamsAbortTriggered) return;
    window._hamsAbortTriggered = true;

    // The magic string that tools/test.py is listening for on stdout
    const msg = `\n[WATCHDOG ALARM] FATAL JS EVENT DETECTED: ${reason}\n${details || ''}\n`;
    originalConsoleError.call(console, msg);

    // Freeze and dump the DOM state for Python to capture
    if (!window._domDumped) {
        window._domDumped = true;
        try {
            let rpcList = window._pendingRPCCount > 0 ? (window._pendingRPCCount + ' pending requests') : 'None';
            let currentHash = document.location.hash || document.location.pathname;
            let stateHeader = `\n========== UI STATE SUMMARY ==========\nURL/Hash: ${currentHash}\nPending RPCs: ${rpcList}\n======================================\n`;
            let skeleton = buildInteractableSkeleton(document.body).replace(/\s{2,}/g, ' ');
            originalConsoleError.call(console, stateHeader + "\n========== INTERACTABLE DOM SKELETON ==========\n" + skeleton + "\n===============================================\n");
        } catch (e) {
            originalConsoleError.call(console, "Failed to dump DOM skeleton: " + e.toString());
        }
    }
}

// =================================================================================
// 2. AGGRESSIVE JAVASCRIPT EVENT HOOKS
// =================================================================================

// Catch native Javascript crashes (undefined is not a function, syntax errors, Owl rendering crashes)
window.addEventListener('error', (event) => {
    if (event.message && BENIGN_MESSAGES.some(benignMsg => event.message.includes(benignMsg))) {
        originalLog.call(console, "[WHITELISTED WINDOW ERROR]", event.message);
        return;
    }

    const trace = event.error ? event.error.stack : 'No stacktrace available';
    triggerInstantAbort("Uncaught Window Error", `${event.message}\n${trace}`);
});

// Catch asynchronous crashes and broken promises (RPC failures, async tour step crashes)
window.addEventListener('unhandledrejection', (event) => {
    let reason = event.reason ? (event.reason.stack || event.reason) : "Unknown Promise Error";
    triggerInstantAbort("Unhandled Promise Rejection", reason);
});

// Intercept network drop completely independent of pending requests
window.addEventListener('offline', () => {
    triggerInstantAbort("Browser Network Offline", "The browser lost connection to the Python backend.");
});

// =================================================================================
// 3. ODOO ENGINE INTERCEPTORS
// =================================================================================

console.error = function (...args) {
    if (isBenign(args)) {
        originalLog.apply(console, ["[WHITELISTED ERROR]"].concat(args));
        return;
    }
    originalConsoleError.apply(console, args);

    const msg = args.map(a => {
        if (typeof a === 'string') return a;
        if (a instanceof Error) return a.stack;
        if (a && a.message) return a.message;
        return '';
    }).join(' ').toLowerCase();

    // Instantly trap Odoo's native framework error patterns
    if (msg.includes('timeout') ||
        msg.includes('failed:') ||
        msg.includes('fatal:') ||
        msg.includes('assertionerror') ||
        msg.includes('tour failed') ||
        msg.includes('step failed')) {
        triggerInstantAbort("Odoo Framework Error", msg);
    }
};

// =================================================================================
// 4. NETWORK RPC TRACKING
// =================================================================================
window._pendingRPCCount = 0;
const originalFetch = window.fetch;

window.fetch = async function(...args) {
    const url = typeof args[0] === 'string' ? args[0] : (args[0] ? args[0].url : 'unknown');
    window._pendingRPCCount++;
    try {
        return await originalFetch.apply(this, args);
    } catch (e) {
        if (e && e.name === 'TypeError' && e.message === 'Failed to fetch') {
            triggerInstantAbort("Fetch API Error", `The backend server crashed or dropped the connection during RPC to: ${url}`);
        }
        throw e;
    } finally {
        window._pendingRPCCount--;
    }
};

const originalXHR = window.XMLHttpRequest.prototype.open;
window.XMLHttpRequest.prototype.open = function(method, url, ...rest) {
    this.addEventListener('loadend', () => window._pendingRPCCount--);
    this.addEventListener('error', () => {
        window._pendingRPCCount--;
        triggerInstantAbort("XHR Network Error", `The backend server crashed or dropped the connection during RPC to: ${url}`);
    });
    this.addEventListener('abort', () => window._pendingRPCCount--);
    window._pendingRPCCount++;
    return originalXHR.call(this, method, url, ...rest);
};

// =================================================================================
// 5. DOM SKELETON BUILDER (Telemetry Compression)
// =================================================================================
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

// Expose to window so Python CDP evaluator can invoke it upon unexpected teardowns
window._buildInteractableSkeleton = buildInteractableSkeleton;

// =================================================================================
// 6. IDLE HANG WATCHDOG (Failsafe)
// =================================================================================
window._hamsTourWatchdog = {
    lastActivity: Date.now(),
    lastLog: "Initialized"
};

console.log = function(...args) {
    originalLog.apply(console, args);
    const msg = args.map(a => typeof a === 'string' ? a : (a && a.message ? a.message : '')).join(' ');

    // Reset idle timer on native Odoo tour progression logs
    if (msg.toLowerCase().includes('tour') || msg.toLowerCase().includes('step') || msg.toLowerCase().includes('trigger')) {
        window._hamsTourWatchdog.lastActivity = Date.now();
        window._hamsTourWatchdog.lastLog = msg;
    }
};

window._hamsTourWatchdogInterval = setInterval(() => {
    if (window._hamsAbortTriggered) {
        clearInterval(window._hamsTourWatchdogInterval);
        return;
    }

    const idleTime = Date.now() - window._hamsTourWatchdog.lastActivity;
    // Trigger alarm if the tour pipeline goes completely silent for 15 seconds
    if (idleTime > 15000) {
        triggerInstantAbort("Idle Hang Watchdog", `Tour step execution idled for over 15 seconds. Last recorded activity: ${window._hamsTourWatchdog.lastLog}`);
    }

    // Detect illegal routing fallback to Discuss app
    const url = document.location.pathname + document.location.hash + document.location.search;
    if (url.includes('/discuss')) {
        triggerInstantAbort("Illegal Routing Override", "Tour illegally redirected to /odoo/discuss! Hash routing or query parameters were malformed.");
    }
}, 2000);
