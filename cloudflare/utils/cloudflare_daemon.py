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

def start_tunnel_daemon(token):
    """
    Starts the Cloudflare tunnel in a background thread using the CGO wrapper.
    Restarts immediately if it crashes, unless explicitly stopped.
    """
    global _tunnel_future
    if _tunnel_future and not _tunnel_future.done():
        _logger.warning("Cloudflare tunnel is already running.")
        return

    lib = _get_lib()
    _stop_event.clear()
    _logger.info("Starting native Cloudflare tunnel daemon...")

    def run_tunnel():
        while not _stop_event.is_set():
            try:
                # We must encode the token as a null-terminated UTF-8 string for C.
                lib.StartTunnel(token.encode('utf-8'))
            except Exception as e:  # audit-ignore-catch-all
                _logger.exception("Cloudflare tunnel daemon crashed: %s", e)
            
            if not _stop_event.is_set():
                _logger.warning("Cloudflare tunnel exited unexpectedly. Restarting immediately...")
                _stop_event.wait(1) # Small pause to prevent tight looping on immediate failure

    _tunnel_future = _tunnel_executor.submit(run_tunnel)

def stop_tunnel_daemon():
    """
    Signals the Cloudflare tunnel to stop.
    """
    if _lib:
        _logger.info("Stopping native Cloudflare tunnel daemon...")
        _stop_event.set()
        _lib.StopTunnel()

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

def stop_tunnel_simulator():
    """
    Stops the native Go HTTPS reverse proxy simulator.
    """
    if _lib:
        _logger.info("Stopping CGO local simulator...")
        _lib.StopLocalSimulator()
