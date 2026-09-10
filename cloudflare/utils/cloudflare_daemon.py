import ctypes
import os
import logging
import threading
from concurrent.futures import ThreadPoolExecutor

_logger = logging.getLogger(__name__)

# Path to the locally built libcloudflared.so
# In production, we'd distribute this binary or install it on the system.
_SO_PATH = os.path.join(
    os.path.dirname(__file__),
    '../../daemons/cloudflared/libcloudflared.so'
)

_lib = None
_tunnel_executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="CloudflareTunnelDaemon")
_tunnel_future = None

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
    _lib.StartTunnel.argtypes = [ctypes.c_char_p]
    _lib.StartLocalSimulator.argtypes = [ctypes.c_int]
    _lib.StartLocalSimulator.restype = ctypes.c_int
    _lib.StopLocalSimulator.argtypes = []
    _lib.StopTunnel.argtypes = []
    return _lib

_stop_event = threading.Event()
# Bug fix (bug-hunt, review_tier 1, 2026-09-09): the "is it already running"
# check and the submit() that starts a new one were two separate statements
# with no lock between them -- two threads in this same process calling
# start_tunnel_daemon() concurrently (e.g. a doubled-click on a "start
# tunnel" button landing on the same worker) could both observe
# `_tunnel_future` as not-yet-running and both proceed to submit a
# run_tunnel() loop, silently losing the first future's own reference and
# queuing a second one behind it. This lock closes that same-process race.
# NOTE: it does NOT close the equivalent race ACROSS Odoo worker PROCESSES
# (each worker has its own independent copy of every module-level global
# here -- `_tunnel_future`, `_lib`, `_stop_event`, `_tunnel_lock` included),
# so two requests landing on two different workers can still each pass this
# check and each start a real, independent native tunnel daemon for the
# same token. Closing that would need a cross-process primitive (a DB row,
# a file lock, a PID file) coordinated with whatever calls this from
# tunnel.py -- out of scope for this file alone; flagged in the bug-hunt
# claim for cloudflare:COMM_start_tunnel_daemon instead of fixed here.
_tunnel_lock = threading.Lock()

def start_tunnel_daemon(token):
    """
    Starts the Cloudflare tunnel in a background thread using the CGO wrapper.
    Restarts immediately if it crashes, unless explicitly stopped.
    """
    global _tunnel_future
    with _tunnel_lock:
        if _tunnel_future and not _tunnel_future.done():
            _logger.warning("Cloudflare tunnel is already running.")
            return

        lib = _get_lib()
        _stop_event.clear()
        _logger.info("Starting native Cloudflare tunnel daemon...")
        # # Verified by [@ANCHOR: COMM_test_edge_traffic_parsing]

        def run_tunnel():
            while not _stop_event.is_set():
                try:
                    # We must encode the token as a null-terminated UTF-8 string for C.
                    # The same native library backs StartTunnel (this real
                    # daemon) and StartLocalSimulator (the test harness), so
                    # WebSocket upgrades proxied through the simulator exercise
                    # the identical native pass-through this call performs.
                    # # Verified by [@ANCHOR: COMM_test_websocket_traffic]
                    lib.StartTunnel(token.encode('utf-8'))
                except Exception as e:  # audit-ignore-catch-all
                    _logger.exception("Cloudflare tunnel daemon crashed: %s", e)

                if not _stop_event.is_set():
                    _logger.warning("Cloudflare tunnel exited unexpectedly. Restarting immediately...")
                    _stop_event.wait(1) # Small pause to prevent tight looping on immediate failure

        # Submitting must stay inside the lock too -- otherwise two threads
        # could both pass the "not running" check above, both release the
        # lock, and both submit here, which is the exact race this lock
        # exists to close.
        _tunnel_future = _tunnel_executor.submit(run_tunnel)

# [@ANCHOR: cloudflare:COMM_stop_tunnel_daemon]
def stop_tunnel_daemon():
    """
    Signals the Cloudflare tunnel to stop.
    """
    if _lib:
        _logger.info("Stopping native Cloudflare tunnel daemon...")
        _stop_event.set()
        _lib.StopTunnel()

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
