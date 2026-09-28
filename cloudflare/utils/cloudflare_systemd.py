# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""
[@ANCHOR: cloudflare:cloudflare_systemd]
Verified by [@ANCHOR: test_cloudflare_systemd]

Replaces cloudflare_daemon.py's old in-process tunnel-lifecycle management (a
ThreadPoolExecutor supervising a CGO-spawned `cloudflared` subprocess, one per tunnel key) with a
real systemd-managed unit. See night_shift_questions/answered/
cloudflare-tunnel-supervisor-architecture-a4e8f1c3.md for the full "why": in short,
`odoo.service`'s own `KillMode=mixed` sends a final cgroup-wide SIGKILL sweep on every restart/stop,
which reaches any subprocess spawned from inside Odoo regardless of process-group tricks -- the
only way to make a tunnel survive an Odoo deploy is to get it out of `odoo.service`'s cgroup
entirely.

Design, settled 2026-09-26:
- A templated systemd --user unit (`cloudflared@.service`, one instance per Cloudflare tunnel id),
  running as the SAME `odoo` OS user (matching `distributed_redis_cache`'s own
  `cache-manager.service` precedent) -- the isolation that matters is the systemd unit boundary
  (a separate cgroup), not the OS user identity, so a new dedicated user would add
  ownership/permission complexity for no additional isolation.
- Controlled via `systemctl --user`, never a system-level unit needing root: a per-user systemd
  instance's units live in `user@<uid>.slice`, already outside `system.slice`/`odoo.service`'s own
  cgroup subtree, so no new sudoers rule or privileged helper is needed for Odoo to manage it.
  Requires `loginctl enable-linger odoo` once, at provisioning time (see
  docs/proposals/PROVISION_PRODUCTION_NOTES.md), so the user manager survives reboots without an
  interactive login.
- The tunnel's run token now touches disk (a deliberate change from the old "fetch live, never
  persist" model, needed so a `Restart=always` unit has something to restart with): written to
  `/opt/hams/etc/keys/cloudflared-<tunnel_key>.env`, matching `daemon_key_manager`'s own existing
  `/opt/hams/etc/keys/` convention for a different credential type.
"""
import logging
import os
import shutil
import subprocess
import tempfile

_logger = logging.getLogger(__name__)

# Same relative-path convention cloudflare_daemon.py's own _BIN_PATH already uses: this file lives
# under cloudflare/utils/, and the real cloudflared binary built from the vendored source lives at
# the repo root's daemons/cloudflared/cloudflared, wherever this particular checkout happens to be
# (a different absolute path on the dev box vs. production).
#
# Found live on hams1, 2026-09-26: this vendored-tree path had gone stale -- something had
# replaced daemons/cloudflared/ with a source-only checkout (no compiled binary) at some point,
# and the only reason the tunnel was still up at all was a single already-running process holding
# the old binary open via a now-deleted inode. Any restart of that one process would have taken
# the tunnel down with no way for either the old or new supervisor to bring it back. Resolving the
# binary at call time (not import time) and falling back to a PATH-installed `cloudflared` (e.g.
# the official apt package) means a missing vendored build no longer silently produces a unit that
# can never start -- it produces either a working fallback or a clearly logged failure instead.
_VENDORED_BIN_PATH = os.path.join(
    os.path.dirname(__file__), "../../daemons/cloudflared/cloudflared"
)


def _resolve_cloudflared_bin():
    """Returns the best available cloudflared binary path: the vendored build if it actually
    exists and is executable, else whatever `cloudflared` is on PATH, else the vendored path
    anyway (so the resulting exec failure is loud and points at the real cause)."""
    vendored = os.path.abspath(_VENDORED_BIN_PATH)
    if os.access(vendored, os.X_OK):
        return vendored
    on_path = shutil.which("cloudflared")
    if on_path:
        _logger.warning(
            "Vendored cloudflared binary not found/executable at %s; falling back to "
            "PATH-installed cloudflared at %s.",
            vendored,
            on_path,
        )
        return on_path
    _logger.error(
        "No usable cloudflared binary found: vendored path %s is missing or not executable, "
        "and no `cloudflared` is on PATH. The rendered systemd unit will fail to start.",
        vendored,
    )
    return vendored
_UNIT_TEMPLATE_PATH = os.path.join(
    os.path.dirname(__file__),
    "../../daemons/cloudflared-ffi/packaging/cloudflared@.service.template",
)

_KEYS_DIR = "/opt/hams/etc/keys"
_USER_UNIT_DIR = os.path.expanduser("~/.config/systemd/user")
_UNIT_NAME_TEMPLATE = "cloudflared@.service"

# Subprocess calls to systemctl/loginctl are all short, local, no-network operations -- a generous
# but bounded timeout catches a genuinely hung systemd (e.g. a D-Bus problem) without risking an
# indefinite hang inside an Odoo request/cron thread.
_SYSTEMCTL_TIMEOUT_SECONDS = 15


def _systemctl_env():
    """`systemctl --user` needs a running user-manager session to talk to (via D-Bus), which
    Odoo's own process -- started by systemd as a SYSTEM unit, never an interactive login -- does
    not get for free: confirmed live, `sudo -u odoo systemctl --user status` fails outright with
    "$DBUS_SESSION_BUS_ADDRESS and $XDG_RUNTIME_DIR not defined" until XDG_RUNTIME_DIR is set
    explicitly (dbus itself then defaults DBUS_SESSION_BUS_ADDRESS to
    unix:path=$XDG_RUNTIME_DIR/bus, so only one of the two needs setting). Computed from
    os.getuid() rather than hardcoded, since the odoo UID differs between hosts; requires
    `loginctl enable-linger odoo` once so /run/user/<uid> and the user manager exist at all
    without an interactive session -- see docs/proposals/PROVISION_PRODUCTION_NOTES.md."""
    env = dict(os.environ)
    env["XDG_RUNTIME_DIR"] = "/run/user/%d" % os.getuid()
    return env


def _run_systemctl(*args):
    """Runs `systemctl --user <args>`, returning (success, stdout_or_stderr)."""
    try:
        result = subprocess.run(
            ["systemctl", "--user", *args],
            capture_output=True,
            text=True,
            timeout=_SYSTEMCTL_TIMEOUT_SECONDS,
            shell=False,
            env=_systemctl_env(),
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        _logger.error("systemctl --user %s failed to run: %s", " ".join(args), e)
        return False, str(e)
    if result.returncode != 0:
        return False, (result.stderr or result.stdout).strip()
    return True, result.stdout.strip()


def _unit_name(tunnel_key):
    return "cloudflared@%s.service" % tunnel_key


def _env_file_path(tunnel_key):
    return os.path.join(_KEYS_DIR, "cloudflared-%s.env" % tunnel_key)


def _write_secure_file(path, content, mandatory_prefix):
    """Writes `content` to `path` at mode 0600, atomically. Same mkstemp + fchmod + os.rename
    idiom `daemon_key_manager`'s own `_write_secure_env_file` already uses (and
    `hams_local_relay/src/lotw.rs`'s `master_key()` independently, in Rust) for the identical
    reason: O_CREAT|O_TRUNC on an existing path can succeed and destroy the previous, possibly
    still-valid content even when the later permission-fixup fails, so this always writes a
    brand-new, correctly-permissioned temp file first and swaps it in atomically."""
    path = os.path.realpath(path)
    # A string prefix is not a directory boundary: "/opt/hams/etc/keys_evil/f" starts with
    # "/opt/hams/etc/keys", and a key containing ".." resolves there. Require the separator.
    if not path.startswith(mandatory_prefix.rstrip(os.sep) + os.sep):
        raise ValueError(
            "Refusing to write outside %s (resolved path: %s)" % (mandatory_prefix, path)
        )
    directory = os.path.dirname(path)
    os.makedirs(directory, mode=0o700, exist_ok=True)
    fd, tmp_path = tempfile.mkstemp(dir=directory, prefix=".cloudflared_")
    try:
        try:
            os.fchmod(fd, 0o600)
        except BaseException:  # audit-ignore-catch-all
            os.close(fd)
            raise
        with os.fdopen(fd, "w") as f:
            f.write(content)
        os.rename(tmp_path, path)
    except BaseException:  # audit-ignore-catch-all
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
        raise


def _remove_env_file(tunnel_key):
    """Removes a stopped tunnel's run-token file, if there is one. The file holds a live credential and
    `start_tunnel_daemon` writes it afresh on every start, so nothing needs it once the unit is disabled. A key that
    would resolve outside the keys directory is refused, the same boundary `_write_secure_file` enforces."""
    path = os.path.realpath(_env_file_path(tunnel_key))
    if not path.startswith(_KEYS_DIR.rstrip(os.sep) + os.sep):
        _logger.error("Refusing to remove a token file outside %s (resolved path: %s)", _KEYS_DIR, path)
        return
    try:
        os.remove(path)
    except FileNotFoundError:
        pass
    except OSError as e:
        _logger.warning("Could not remove the token file for tunnel %s: %s", tunnel_key, e)


# [@ANCHOR: cloudflare:ensure_unit_installed]
def _ensure_unit_installed():
    """Renders the template unit with this host's own resolved cloudflared binary path and
    writes it to the odoo user's systemd --user unit directory, reloading only when the
    rendered content actually changed (daemon-reload is cheap but unnecessary on every call)."""
    with open(_UNIT_TEMPLATE_PATH, "r") as f:
        template = f.read()
    # A literal .replace(), not str.format(): the template's own explanatory comments can contain
    # curly-brace text, which .format() would choke on or mis-substitute (confirmed live when an
    # earlier version of this template's comment, which happened to mention the placeholder's name in
    # {curly braces} too, got silently rewritten by .format() right alongside the real ExecStart line).
    rendered = template.replace("{cloudflared_bin}", _resolve_cloudflared_bin())

    unit_path = os.path.join(_USER_UNIT_DIR, _UNIT_NAME_TEMPLATE)
    existing = None
    if os.path.exists(unit_path):
        with open(unit_path, "r") as f:
            existing = f.read()
    if existing == rendered:
        return

    os.makedirs(_USER_UNIT_DIR, mode=0o700, exist_ok=True)
    fd, tmp_path = tempfile.mkstemp(dir=_USER_UNIT_DIR, prefix=".cloudflared_unit_")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(rendered)
        os.rename(tmp_path, unit_path)
    except BaseException:  # audit-ignore-catch-all
        if os.path.exists(tmp_path):
            os.remove(tmp_path)
        raise
    success, output = _run_systemctl("daemon-reload")
    if not success:
        _logger.error("systemctl --user daemon-reload failed: %s", output)


# [@ANCHOR: cloudflare:is_tunnel_daemon_running]
def is_tunnel_daemon_running(tunnel_key):
    """True when `cloudflared@<tunnel_key>.service` is currently active. Mirrors the old
    module-level function's name/signature so tunnel.py's own calling convention is unaffected."""
    success, output = _run_systemctl("is-active", _unit_name(tunnel_key))
    return success and output == "active"


# [@ANCHOR: cloudflare:start_tunnel_daemon]
def start_tunnel_daemon(token, tunnel_key):
    """Writes this tunnel's run token to its own EnvironmentFile and enables+starts its systemd
    --user unit. Idempotent: `enable --now` on an already-enabled, already-running unit is a
    no-op, so this is safe to call on every cron tick the same way the old executor-based
    version's own idempotency was."""
    _ensure_unit_installed()
    _write_secure_file(
        _env_file_path(tunnel_key),
        "# Auto-generated by cloudflare.tunnel -- do not edit by hand.\n"
        "TUNNEL_TOKEN=%s\n" % token,
        _KEYS_DIR,
    )
    success, output = _run_systemctl("enable", "--now", _unit_name(tunnel_key))
    if not success:
        _logger.error(
            "Failed to start Cloudflare tunnel unit for %s: %s", tunnel_key, output
        )
        return False
    _logger.info("Started/ensured Cloudflare tunnel systemd unit for %s.", tunnel_key)
    return True


# [@ANCHOR: cloudflare:stop_tunnel_daemon]
def stop_tunnel_daemon(tunnel_key=None):
    """Stops one tunnel's unit, or every currently-known `cloudflared@*` unit when `tunnel_key`
    is None. Unlike the old in-process version, there is no module-level registry of "keys this
    process started" to iterate -- units are discovered live via `systemctl --user list-units`,
    which is authoritative regardless of which Odoo worker (or none) last touched them."""
    if tunnel_key is not None:
        success, output = _run_systemctl("disable", "--now", _unit_name(tunnel_key))
        if not success:
            _logger.error(
                "Failed to stop Cloudflare tunnel unit for %s: %s", tunnel_key, output
            )
            return
        _remove_env_file(tunnel_key)
        return

    success, output = _run_systemctl(
        "list-units", "cloudflared@*.service", "--plain", "--no-legend", "--all"
    )
    if not success:
        _logger.error("Failed to list Cloudflare tunnel units: %s", output)
        return
    for line in output.splitlines():
        unit = line.split()[0] if line.split() else ""
        if unit.startswith("cloudflared@") and unit.endswith(".service"):
            disabled, _ = _run_systemctl("disable", "--now", unit)
            if disabled:
                _remove_env_file(unit[len("cloudflared@"):-len(".service")])
