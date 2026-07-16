import ctypes
import os
import logging
from concurrent.futures import ThreadPoolExecutor

_logger = logging.getLogger(__name__)

# Path to the locally built libcloudflared.so
# In production, we'd distribute this binary or install it on the system.
_SO_PATH = os.path.join(
    os.path.dirname(__file__),
    '../../../daemons/cloudflared/libcloudflared.so'
)

_lib = None
_tunnel_executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="CloudflareTunnelDaemon")
_tunnel_future = None

def _load_lib():
    global _lib
    if _lib is None:
        if not os.path.exists(_SO_PATH):
            raise FileNotFoundError(f"Cannot find libcloudflared.so at {_SO_PATH}")
        _lib = ctypes.cdll.LoadLibrary(_SO_PATH)
        _lib.StartTunnel.argtypes = [ctypes.c_char_p]
        _lib.StopTunnel.argtypes = []
    return _lib

def start_tunnel_daemon(token):
    """
    Starts the Cloudflare tunnel in a background thread using the CGO wrapper.
    """
    global _tunnel_future
    if _tunnel_future and not _tunnel_future.done():
        _logger.warning("Cloudflare tunnel is already running.")
        return

    lib = _load_lib()
    _logger.info("Starting native Cloudflare tunnel daemon...")

    def run_tunnel():
        try:
            # We must encode the token as a null-terminated UTF-8 string for C.
            lib.StartTunnel(token.encode('utf-8'))
        except Exception as e:  # audit-ignore-catch-all
            _logger.exception("Cloudflare tunnel daemon crashed: %s", e)

    _tunnel_future = _tunnel_executor.submit(run_tunnel)

def stop_tunnel_daemon():
    """
    Signals the Cloudflare tunnel to stop.
    """
    if _lib:
        _logger.info("Stopping native Cloudflare tunnel daemon...")
        _lib.StopTunnel()
