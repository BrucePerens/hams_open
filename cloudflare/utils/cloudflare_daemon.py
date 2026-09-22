import ctypes
import os
import logging
import threading
from concurrent.futures import ThreadPoolExecutor

_logger = logging.getLogger(__name__)

# Path to the locally built libcloudflared.so
# In production, we'd distribute this binary or install it on the system.
# Bug fix (night-watch, 2026-09-17, per Bruce -- "restore the source,
# deleting it was a mistake"): this pointed at daemons/cloudflared/ (the
# large vendored upstream cloudflared CLI tree) from 2026-08-25 to
# 2026-09-17, because that's where StartLocalSimulator/StopLocalSimulator
# actually lived at the time -- the 2026-08-25 licensing audit (51b81cb3)
# deleted this directory's own main.go/go.mod/go.sum/README.md (and its
# own .so/.h) for lacking a license header, without noticing this Python
# path still needed a buildable source for the functions it loads.
# Restored here with a proper SPDX header (daemons/cloudflared-ffi/
# README.md), so this points at its own source again rather than at an
# unrelated vendored tree's build output.
_SO_PATH = os.path.join(
    os.path.dirname(__file__),
    '../../daemons/cloudflared-ffi/libcloudflared.so'
)

# The real, working `cloudflared` binary built from daemons/cloudflared (see that
# directory's own main.go for why StartTunnel below needs it as a subprocess rather
# than an in-process call). Same relative-path convention as _SO_PATH above.
_BIN_PATH = os.path.join(
    os.path.dirname(__file__),
    '../../daemons/cloudflared/cloudflared'
)

_lib = None

# The key every piece of per-tunnel state below is filed under when a caller
# does not name one. tunnel.py always names one (the Cloudflare tunnel id); the
# default exists for the direct, single-tunnel callers that predate multi-tunnel
# support.
DEFAULT_TUNNEL_KEY = "default"

# One Odoo server fronting several websites is the common case, not the exotic
# one (Bruce, 2026-09-19, answering
# hams_com/night_shift_questions/answered/
# cloudflare-tunnel-ensure-running-multi-tunnel-scope-ec6882e6.md: "one server
# fronting multiple web sites is the common case ... So, make that work
# correctly"), so the daemon state here is keyed PER TUNNEL instead of living in
# a single module-level slot.
#
# This replaced ONE shared `ThreadPoolExecutor(max_workers=1)` and one
# `_tunnel_future`. That was not merely single-tunnel by convention, it was
# single-tunnel by construction: `run_tunnel()` never returns, so a second
# `submit()` on that shared executor would have sat in its queue forever behind
# the first tunnel and only ever started if the first one stopped -- a silent
# failure, on a queue nothing inspects. One single-worker executor PER TUNNEL
# has no such queue, while keeping each tunnel's own thread count bounded at one
# (the reason this is an executor rather than a bare `threading.Thread`, which
# the repo's burn list rejects outright as an unbounded-thread DOS vector).
#
# The KEY is the Cloudflare tunnel id: an opaque, non-secret identifier that is
# safe to log and to name a thread after. The tunnel's RUN TOKEN is never a key,
# never logged, and never stored -- it arrives as a Python argument and goes
# straight into `.encode('utf-8')` for the C call, which is exactly as far as it
# travelled before this change.
_tunnel_executors = {}
_tunnel_futures = {}
_tunnel_stop_events = {}

# [@ANCHOR: cloudflare:COMM_get_lib]
def _get_lib():
    global _lib
    if _lib is not None:
        return _lib
        
    lib_path = os.path.abspath(_SO_PATH)
    try:
        _lib = ctypes.CDLL(lib_path)
    except OSError as e:
        _logger.error(f"Failed to load libcloudflared.so: {e}")
        raise RuntimeError(f"Failed to load libcloudflared.so: {e}")

    # Define the argument types for the C functions
    _lib.StartTunnel.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p]
    _lib.StartLocalSimulator.argtypes = [ctypes.c_int]
    _lib.StartLocalSimulator.restype = ctypes.c_int
    _lib.StopLocalSimulator.argtypes = []
    _lib.StopTunnel.argtypes = [ctypes.c_char_p]
    return _lib

# Bug fix (bug-hunt, review_tier 1, 2026-09-09): the "is it already running"
# check and the submit() that starts a new one were two separate statements
# with no lock between them -- two threads in this same process calling
# start_tunnel_daemon() concurrently (e.g. a doubled-click on a "start
# tunnel" button landing on the same worker) could both observe
# `_tunnel_future` as not-yet-running and both proceed to submit a
# run_tunnel() loop, silently losing the first future's own reference and
# queuing a second one behind it. This lock closes that same-process race.
# It still does, per tunnel key, now that the single `_tunnel_future` has
# become the `_tunnel_futures` dict: the check and the submit() are both
# inside it, so two callers racing on the SAME key cannot both start.
# Different keys are independent by construction, which is the point.
# NOTE: it does NOT close the equivalent race ACROSS Odoo worker PROCESSES
# (each worker has its own independent copy of every module-level global
# here -- `_tunnel_futures`, `_lib`, `_tunnel_stop_events`, `_tunnel_lock`
# included), so two requests landing on two different workers can still each
# start a real, independent native tunnel daemon for the same token. Closing
# that would need a cross-process primitive (a DB row, a file lock, a PID
# file) coordinated with whatever calls this from tunnel.py -- out of scope
# for this file alone; flagged in the bug-hunt claim for
# cloudflare:COMM_start_tunnel_daemon instead of fixed here.
_tunnel_lock = threading.Lock()


# [@ANCHOR: cloudflare:COMM_is_tunnel_daemon_running]
def is_tunnel_daemon_running(tunnel_key=DEFAULT_TUNNEL_KEY):
    """True when this process already has a live daemon loop for `tunnel_key`.

    The caller tunnel.py uses this to decide whether a tunnel needs anything
    doing at all, so that a tunnel already running does not cost a Cloudflare
    API round trip on every five-minute cron tick -- and so that "an already
    running tunnel is not restarted" is a fact a test can assert directly
    rather than infer from the absence of a side effect.
    """
    with _tunnel_lock:
        future = _tunnel_futures.get(tunnel_key)
        return bool(future is not None and not future.done())


def start_tunnel_daemon(token, tunnel_key=DEFAULT_TUNNEL_KEY):
    """
    Starts one Cloudflare tunnel on its own single-worker executor using the
    CGO wrapper. Restarts immediately if it crashes, unless explicitly stopped.

    Idempotent per `tunnel_key`: a key whose loop is still running is left
    alone (and the call returns False), so re-running "ensure the tunnels are
    up" never doubles a daemon. A key whose loop has ENDED is started again on
    that same key's executor, which is what makes this an "ensure", not a
    "start once".

    `token` is the tunnel's run token. It is deliberately not part of the key,
    is never logged, and never leaves this function except as the UTF-8 bytes
    handed to the C entry point.
    """
    with _tunnel_lock:
        existing = _tunnel_futures.get(tunnel_key)
        if existing is not None and not existing.done():
            _logger.warning("Cloudflare tunnel %s is already running.", tunnel_key)
            return False

        lib = _get_lib()
        bin_path = os.path.abspath(_BIN_PATH).encode('utf-8')
        # A fresh Event per start, rather than .clear() on a shared one: a
        # stop_tunnel_daemon() for THIS key must not be able to reach into a
        # later start's loop, and a stop for ANOTHER key must not reach into
        # this one at all -- which is what the single module-level
        # `_stop_event` did by construction before this change.
        stop_event = threading.Event()
        _tunnel_stop_events[tunnel_key] = stop_event
        _logger.info("Starting native Cloudflare tunnel daemon for %s...", tunnel_key)
        # # Verified by [@ANCHOR: COMM_test_edge_traffic_parsing]

        def run_tunnel():
            while not stop_event.is_set():
                try:
                    # We must encode the token as a null-terminated UTF-8 string for C.
                    # The same native library backs StartTunnel (this real
                    # daemon) and StartLocalSimulator (the test harness), so
                    # WebSocket upgrades proxied through the simulator exercise
                    # the identical native pass-through this call performs.
                    # # Verified by [@ANCHOR: COMM_test_websocket_traffic]
                    lib.StartTunnel(tunnel_key.encode('utf-8'), token.encode('utf-8'), bin_path)
                except Exception as e:  # audit-ignore-catch-all
                    _logger.exception(
                        "Cloudflare tunnel daemon %s crashed: %s", tunnel_key, e
                    )

                if not stop_event.is_set():
                    _logger.warning(
                        "Cloudflare tunnel %s exited unexpectedly. Restarting immediately...",
                        tunnel_key,
                    )
                    stop_event.wait(1) # Small pause to prevent tight looping on immediate failure

        # Submitting must stay inside the lock too -- otherwise two threads
        # could both pass the "not running" check above, both release the
        # lock, and both submit here, which is the exact race this lock
        # exists to close.
        #
        # The executor is created once per key and REUSED across restarts of
        # that same tunnel: its single worker is free again the moment the
        # previous run_tunnel() returned, and reusing it means a tunnel that
        # flaps cannot accumulate one abandoned executor per restart.
        executor = _tunnel_executors.get(tunnel_key)
        if executor is None:
            executor = ThreadPoolExecutor(
                max_workers=1,
                thread_name_prefix="CloudflareTunnelDaemon-%s" % tunnel_key,
            )
            _tunnel_executors[tunnel_key] = executor
        _tunnel_futures[tunnel_key] = executor.submit(run_tunnel)
        return True

# [@ANCHOR: cloudflare:COMM_stop_tunnel_daemon]
def stop_tunnel_daemon(tunnel_key=None):
    """
    Signals Cloudflare tunnel daemons to stop: one named `tunnel_key`, or
    every one this process started when `tunnel_key` is None (the default,
    and the behavior every pre-existing caller already relied on).

    Bug fix (2026-09-22): the native library's `StopTunnel()` used to take no
    tunnel handle -- there was exactly one native tunnel, process-wide, by
    construction (StartTunnel never actually started anything real, so a
    single global slot was never exercised) -- so a single-tunnel stop only
    ever signalled that tunnel's own Python loop and left the native call
    alone, "because making it here would reach into every OTHER tunnel's
    daemon". Now that StartTunnel runs each tunnel as its own real subprocess
    keyed by `tunnel_key` (daemons/cloudflared-ffi/main.go), StopTunnel takes
    that same key and only ever reaches the one subprocess named by it, so a
    single-tunnel stop can and does signal the real native process too.
    """
    if tunnel_key is not None:
        stop_event = _tunnel_stop_events.get(tunnel_key)
        if stop_event is not None:
            _logger.info(
                "Stopping native Cloudflare tunnel daemon for %s...", tunnel_key
            )
            stop_event.set()
        if _lib:
            _lib.StopTunnel(tunnel_key.encode('utf-8'))
        return

    if _lib:
        _logger.info("Stopping every native Cloudflare tunnel daemon...")
        for key, stop_event in list(_tunnel_stop_events.items()):
            stop_event.set()
            _lib.StopTunnel(key.encode('utf-8'))

# [@ANCHOR: cloudflare:COMM_start_tunnel_simulator]
def start_tunnel_simulator(target_port):
    """
    Starts the native Go HTTPS reverse proxy simulator.
    Returns the dynamic OS-assigned port it binds to.
    """
    lib = _get_lib()
    _logger.info(f"Starting CGO local simulator targeting port {target_port}...")
    bound_port = lib.StartLocalSimulator(target_port)
    if bound_port < 0:
        raise RuntimeError("Failed to start CGO local simulator.")
    return bound_port

# [@ANCHOR: cloudflare:COMM_stop_tunnel_simulator]
def stop_tunnel_simulator():
    """
    Stops the native Go HTTPS reverse proxy simulator.
    """
    if _lib:
        _logger.info("Stopping CGO local simulator...")
        _lib.StopLocalSimulator()
