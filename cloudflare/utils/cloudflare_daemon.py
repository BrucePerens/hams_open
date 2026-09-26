import ctypes
import os
import logging

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

_lib = None


# [@ANCHOR: cloudflare:COMM_get_lib]
def _get_lib():
    """Loads libcloudflared.so, used only by the local HTTPS simulator below now.

    2026-09-26: this module's own real-tunnel lifecycle (StartTunnel/StopTunnel, the
    ThreadPoolExecutor-per-key supervisor, the cross-process flock) moved to
    cloudflare_systemd.py -- see that module's own docstring and
    night_shift_questions/answered/cloudflare-tunnel-supervisor-architecture-a4e8f1c3.md for why
    (odoo.service's own KillMode=mixed kills anything in its cgroup on every restart, which no
    in-process fix can escape). StartTunnel/StopTunnel still exist on the Go side
    (daemons/cloudflared-ffi/main.go) but are no longer called from Python; left in place rather
    than deleted in the same change, since removing an //export'd CGO function needs its own
    careful pass (rebuilding the .so, regenerating libcloudflared.h, confirming nothing else
    references them) that's out of scope for this refactor.
    """
    global _lib
    if _lib is not None:
        return _lib

    lib_path = os.path.abspath(_SO_PATH)
    try:
        _lib = ctypes.CDLL(lib_path)
    except OSError as e:
        _logger.error(f"Failed to load libcloudflared.so: {e}")
        raise RuntimeError(f"Failed to load libcloudflared.so: {e}")

    # Define the argument types for the C functions this module still calls.
    _lib.StartLocalSimulator.argtypes = [ctypes.c_int]
    _lib.StartLocalSimulator.restype = ctypes.c_int
    _lib.StopLocalSimulator.argtypes = []
    return _lib


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
