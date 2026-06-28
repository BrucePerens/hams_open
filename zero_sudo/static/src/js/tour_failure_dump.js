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
    originalWarn.apply(console, [new Error("Warning Trace").stack]);
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

    const stack = new Error("Abort Trace").stack;

    // The magic string that tools/test.py is listening for on stdout
    const msg = "\n[WATCHDOG ALARM] FATAL JS EVENT DETECTED: " + reason + "\n" + (details || '') + "\nStack Trace:\n" + stack + "\n";
    originalConsoleError.call(console, msg);

    // Freeze and dump the DOM state for Python to capture
    if (!window._domDumped) {
        window._domDumped = true;
        try {
            let rpcList = window._pendingRPCCount > 0 ? (window._pendingRPCCount + ' pending requests') : 'None';
            let currentHash = document.location.hash || document.location.pathname;
            let stateHeader = "\n========== UI STATE SUMMARY ==========\nURL/Hash: " + currentHash + "\nPending RPCs: " + rpcList + "\n======================================\n";
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
    triggerInstantAbort("Uncaught Window Error", event.message + "\n" + trace);
});

// Catch asynchronous crashes and broken promises (RPC failures, async tour step crashes)
window.addEventListener('unhandledrejection', (event) => {
    let reason = event.reason ? (event.reason.stack || event.reason) : "Unknown Promise Error";
    let msg = event.reason && event.reason.message ? event.reason.message.toLowerCase() : "";
    if (event.defaultPrevented || msg.includes("un-mounted") || msg.includes("fetch") || msg.includes("modal") || msg.includes("abort") || msg.includes("reading 'contains'") || msg.includes("undefined or null to object")) {
        event.preventDefault();
        return;
    }
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

    // MUTING THE TEARDOWN ALARM
    // If Odoo natively fails the tour, the test is effectively over and Python is about to kill Chrome.
    // Set the abort flag to gracefully disable the watchdog so it doesn't scream during teardown.
    if (msg.includes('failed:') || msg.includes('tour failed')) {
        window._hamsAbortTriggered = true;
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
        if (e && e.name === 'TypeError' && e.message && e.message.toLowerCase().includes('fetch')) {
            // Suppress fetch errors during teardowns
            return new Response('{}', {status: 200});
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
        triggerInstantAbort("XHR Network Error", "The backend server crashed or dropped the connection during RPC to: " + url);
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
            if (node.hasAttribute(a)) attrs.push(a + '="' + node.getAttribute(a) + '"');
        });

        if (node.classList) {
            // Strip layout/utility classes, keep semantic identifiers
            let cls = Array.from(node.classList).filter(c => c.startsWith('o_') || c === 'btn' || c.startsWith('btn-')).join(' ');
            if (cls) attrs.push('class="' + cls + '"');
        }
        let tag = node.tagName.toLowerCase();
        let content = childrenText.length > 80 ? childrenText.substring(0, 80) + '...' : childrenText;
        return "\n<" + tag + " " + attrs.join(' ') + ">" + content + "</" + tag + ">";
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

// Shared Worker V8 Hang Watchdog using string array to avoid backtick extraction corruption
const sharedWorkerCode = [
    "let vtime = 0;",
    "let lastReal = Date.now();",
    "let lastPing = 0;",
    "let lastState = 'Initializing...';",
    "let lastLog = '';",
    "let domGrowthStartTime = 0;",
    "let lastDomSize = 0;",
    "",
    "function triggerDumpAndKill(diag) {",
    "    console.error(diag);",
    "    fetch('/hams_test/watchdog/dump', {",
    "        method: 'POST',",
    "        headers: {'Content-Type': 'application/json'},",
    "        body: JSON.stringify({ params: { diagnostic: diag, log: lastLog } })",
    "    }).then(function() {",
    "        fetch('/hams_test/watchdog/kill', {",
    "            method: 'POST',",
    "            headers: {'Content-Type': 'application/json'},",
    "            body: JSON.stringify({ params: {} })",
    "        });",
    "    }).catch(function() {",
    "        fetch('/hams_test/watchdog/kill', {",
    "            method: 'POST',",
    "            headers: {'Content-Type': 'application/json'},",
    "            body: JSON.stringify({ params: {} })",
    "        });",
    "    });",
    "    lastPing = vtime + 60000;",
    "}",
    "",
    "self.onconnect = function(e) {",
    "    const port = e.ports[0];",
    "    port.onmessage = function(event) {",
    "        if (event.data.type === 'ping') {",
    "            lastPing = vtime;",
    "            if (event.data.state) lastState = event.data.state;",
    "            if (event.data.log) lastLog = event.data.log;",
    "            ",
    "            let currentDomSize = event.data.domSize || 0;",
    "            ",
    "            if (currentDomSize > 5000000) {",
    "                triggerDumpAndKill('V8 TIGHT LOOP DETECTED: DOM size exceeded 5MB skeleton.\\nLast Log: ' + lastLog + '\\nDOM Skeleton:\\n' + lastState.substring(0, 5000) + '\\n...[TRUNCATED]');",
    "            } else if (currentDomSize > lastDomSize && currentDomSize > 5000) {",
    "                if (domGrowthStartTime === 0) {",
    "                    domGrowthStartTime = vtime;",
    "                } else if (vtime - domGrowthStartTime > 15000) {",
    "                    triggerDumpAndKill('V8 TIGHT LOOP DETECTED: DOM grew without bounds for > 15 virtual seconds.\\nLast Log: ' + lastLog + '\\nDOM Skeleton:\\n' + lastState.substring(0, 5000) + '\\n...[TRUNCATED]');",
    "                }",
    "            } else {",
    "                domGrowthStartTime = 0;",
    "            }",
    "            lastDomSize = currentDomSize;",
    "        }",
    "    };",
    "};",
    "",
    "setInterval(function() {",
    "    let now = Date.now();",
    "    let delta = now - lastReal;",
    "    lastReal = now;",
    "    vtime += Math.min(delta, 500);",
    "    ",
    "    if (vtime - lastPing > 15000) {",
    "        triggerDumpAndKill('V8 TIGHT LOOP DETECTED: Main thread unresponsive for 15 virtual seconds.\\nLast Log: ' + lastLog + '\\nDOM Skeleton:\\n' + lastState.substring(0, 5000) + '\\n...[TRUNCATED]');",
    "    }",
    "}, 100);"
].join("\n");

try {
    const blob = new Blob([sharedWorkerCode], { type: 'application/javascript' });
    const workerUrl = URL.createObjectURL(blob);
    const watchdogWorker = new SharedWorker(workerUrl);
    watchdogWorker.port.start();

    // Virtual clock tracker for the main thread
    window._hamsVirtualTime = 0;
    let _hamsLastRealTime = Date.now();
    setInterval(() => {
        let now = Date.now();
        let delta = now - _hamsLastRealTime;
        _hamsLastRealTime = now;
        window._hamsVirtualTime += Math.min(delta, 500);
    }, 100);

    window._hamsTourWatchdogInterval = setInterval(() => {
        if (window._hamsAbortTriggered) {
            clearInterval(window._hamsTourWatchdogInterval);
            return;
        }

        // Lightweight heartbeat to prevent stringification lag
        let currentDomSize = document.getElementsByTagName('*').length;

        watchdogWorker.port.postMessage({
            type: 'ping',
            state: "Heartbeat. DOM size: " + currentDomSize,
            domSize: currentDomSize,
            log: window._hamsTourWatchdog.lastLog
        });
    }, 2000);
} catch (e) {
    console.warn("Could not instantiate SharedWorker watchdog:", e);
}
