#!/usr/bin/env python3
"""
Infrastructure Blueprint & Provisioning Engine
Serves as the Single Source of Truth for test.py and deploy_wizard.py.
Supports environment scoping, lifecycle hooks, and precise runtime mount states.
"""

import compileall
import contextlib
import glob
import grp
import logging
import os
import pwd
import shutil
import subprocess
import sys
import urllib.request

_logger = logging.getLogger(__name__)

def get_pg_bin(name):
    """Locates PostgreSQL binaries dynamically across installed versions."""
    paths = glob.glob(f"/usr/lib/postgresql/*/bin/{name}")
    if paths:
        return sorted(paths)[-1]
    res = shutil.which(name)
    if not res:
        for p in [f"/usr/bin/{name}", f"/usr/local/bin/{name}"]:
            if os.path.exists(p): return p
        raise FileNotFoundError(f"Could not find PostgreSQL binary: {name}")
    return res

def get_os_identifier():
    try:
        with open("/etc/os-release") as f:
            for line in f:
                if line.startswith("ID="):
                    return line.strip().split("=")[1].strip('"').lower()
    except OSError as e:
        _logger.debug("Ignored OSError reading /etc/os-release: %s", e)
    return "ubuntu"

@contextlib.contextmanager
def micro_privilege(username):
    """
    Temporarily drops Effective privileges to the specified user using setresuid/setresgid.
    Restores Root privileges securely upon exiting the context block.
    """
    if os.geteuid() != 0:
        yield
        return

    user_info = pwd.getpwnam(username)
    target_uid = user_info.pw_uid
    target_gid = user_info.pw_gid

    orig_ruid, orig_euid, orig_suid = os.getresuid()
    orig_rgid, orig_egid, orig_sgid = os.getresgid()

    try:
        os.setresgid(orig_rgid, target_gid, orig_sgid)
        os.setresuid(orig_ruid, target_uid, orig_suid)
        yield
    finally:
        os.setresuid(orig_ruid, orig_euid, orig_suid)
        os.setresgid(orig_rgid, orig_egid, orig_sgid)


def format_env(text, env_vars):
    if not text: return ""
    try:
        return text.format(**(env_vars or {}))
    except KeyError as e:
        _logger.debug("KeyError formatting %s: %s", text, e)
        return text

def safe_remove(path):
    if os.path.exists(path):
        try:
            os.remove(path)
        except OSError as e:
            _logger.debug("OSError removing file: %s", e)

def apply_permissions(path, owner_str, mode_int, recursive=False):
    uid, gid = -1, -1
    if owner_str:
        try:
            user, group = owner_str.split(":")
            uid = pwd.getpwnam(user).pw_uid
            gid = grp.getgrnam(group).gr_gid
        except KeyError as e:
            _logger.warning("User/Group %s not found: %s", owner_str, e)

    def _apply(p):
        try:
            if uid != -1 and gid != -1:
                os.chown(p, uid, gid)
            if mode_int is not None:
                os.chmod(p, mode_int)
        except OSError as e:
            _logger.debug("Failed chown/chmod on %s: %s", p, e)

    _apply(path)
    if recursive and os.path.isdir(path):
        for root, dirs, files in os.walk(path):
            for item in dirs + files:
                _apply(os.path.join(root, item))

def download_file(url, path, mode, env_vars):
    ua = env_vars.get("SYSTEM_USER_AGENT", "Hams.com Bruce Perens K6BP <bruce@perens.com> +1 510-394-5627")
    req = urllib.request.Request(url, headers={"User-Agent": ua})
    try:
        with urllib.request.urlopen(req) as response:
            data = response.read()
    except Exception as e: # audit-ignore-catch-all
        _logger.warning("Network partition fallback safety hit fetching %s: %s", url, e)
        data = b""

    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
    fd = os.open(path, flags, mode)
    with open(fd, "wb") as f:
        f.write(data)


def hook_generate_ssl(env_vars, dest_dir, path, run_cmd_func):
    domain = env_vars.get('DOMAIN', 'localhost')
    ssl_dir = os.path.join(dest_dir, path.lstrip('/')) if dest_dir else path
    fullchain = os.path.join(ssl_dir, 'fullchain.pem')
    privkey = os.path.join(ssl_dir, 'privkey.pem')
    lotw = os.path.join(ssl_dir, 'lotw_root.pem')
    if not os.path.exists(fullchain):
        try:
            run_cmd_func(['openssl', 'req', '-x509', '-nodes', '-days', '3650', '-newkey', 'rsa:2048', '-keyout', privkey, '-out', fullchain, '-subj', f'/C=US/ST=CA/L=SF/O=Hams/CN={domain}'], stderr=subprocess.DEVNULL)
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Failed to generate SSL certs: %s", e)
        if os.path.exists(fullchain):
            shutil.copy2(fullchain, lotw)


def hook_clear_pycache(env_vars, dest_dir, path, run_cmd_func):
    pycache = os.path.join(dest_dir, 'opt/hams/pycache') if dest_dir else '/opt/hams/pycache'
    daemons = os.path.join(dest_dir, 'opt/hams/daemons') if dest_dir else '/opt/hams/daemons'
    if os.path.exists(pycache):
        for item in os.listdir(pycache):
            item_path = os.path.join(pycache, item)
            shutil.rmtree(item_path, ignore_errors=True) if os.path.isdir(item_path) else safe_remove(item_path)
    if os.path.isdir(daemons):
        compileall.compile_dir(daemons, quiet=1)


def hook_install_odoo_key(env_vars, dest_dir, path, run_cmd_func):
    out = os.path.join(dest_dir, 'usr/share/keyrings/odoo-archive-keyring.gpg') if dest_dir else '/usr/share/keyrings/odoo-archive-keyring.gpg'
    os.makedirs(os.path.dirname(out), exist_ok=True)
    run_cmd_func(['gpg', '--dearmor', '-o', out, '--yes', path])
    safe_remove(path)


def hook_install_kopia_binary(env_vars, dest_dir, path, run_cmd_func):
    try:
        target_dir = os.path.join(dest_dir, 'usr/bin') if dest_dir else '/usr/bin'
        os.makedirs(target_dir, exist_ok=True)
        run_cmd_func(['tar', '-xzf', path, '-C', target_dir, '--strip-components=1', 'kopia-0.23.0-linux-x64/kopia'])
        run_cmd_func(['chmod', '+x', os.path.join(target_dir, 'kopia')])
    except Exception as e: # audit-ignore-catch-all
        _logger.warning("Kopia binary install failed: %s", e)
    safe_remove(path)


def hook_install_cloudflared(env_vars, dest_dir, path, run_cmd_func):
    token = env_vars.get('CLOUDFLARE_TUNNEL_TOKEN')
    run_cmd_func(['dpkg', '-i', path])
    if token:
        run_cmd_func(['cloudflared', 'service', 'install', token])
    safe_remove(path)


def hook_install_pypdf2(env_vars, dest_dir, path, run_cmd_func):
    try:
        run_cmd_func(['/usr/bin/python3', '-m', 'pip', 'install', '--break-system-packages', 'PyPDF2==2.12.1'])
    except Exception as e: # audit-ignore-catch-all
        _logger.warning("PyPDF2 pip install failed: %s", e)
    safe_remove(path)


MANIFEST = {
    "system_accounts": [
        {
            "user": "hams_com",
            "group": "hams_com",
            "home": "/opt/hams",
            "shell": "/bin/bash",
            "add_to_users": ["odoo"],
            "environments": ["prod", "test"],
        }
    ],
    "directories": [
        {
            "path": "/opt/hams",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/etc",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/etc/keys",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/nginx",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod"],
        },
        {
            "path": "/opt/hams/nginx/ssl",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod"],
            "post_provision_hooks": [hook_generate_ssl],
        },
        {
            "path": "/deploy/ssl",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["docker"],
            "post_provision_hooks": [hook_generate_ssl],
        },
        {
            "path": "/opt/hams/odoo",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/cache",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/cache/ms-playwright",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/pycache",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
            "post_provision_hooks": [hook_clear_pycache],
        },
        {
            "path": "/opt/hams/spool",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/spool/adif_queue",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/spool/ncvec",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/failed_input",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/downloads",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/test",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["test"],
        },
        {
            "path": "/var/lib/odoo",
            "owner": "odoo:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local",
            "owner": "odoo:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local/share",
            "owner": "odoo:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local/share/Odoo",
            "owner": "odoo:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local/share/Odoo/sessions",
            "owner": "odoo:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/daemon_keys",
            "owner": "odoo:hams_com",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/backups",
            "owner": "odoo:hams_com",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/rabbitmq",
            "owner": "rabbitmq:rabbitmq",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/log/odoo",
            "owner": "odoo:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/powerdns",
            "owner": "pdns:pdns",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/odoo.service.d",
            "owner": "hams_com:hams_com",
            "provision_mode": "750",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/log/redis",
            "owner": "redis:redis",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/log/rabbitmq",
            "owner": "rabbitmq:rabbitmq",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/redis",
            "owner": "redis:redis",
            "provision_mode": "750",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/tmp/odoo_test_home",
            "owner": "hams_com:hams_com",
            "provision_mode": "770",
            "runtime_mount": "rw",
            "environments": ["test"],
        },
    ],
    "env_groups": {
        "db.env": [
            "DB_NAME",
            "POSTGRES_PASSWORD",
            "DB_PASS",
            "DB_HOST",
            "DB_PORT",
            "DB_USER",
        ],
        "pdns.env": ["PDNS_API_KEY", "PDNS_API_URL"],
        "odoo.env": [
            "ODOO_ADMIN_PASSWORD",
            "ODOO_SERVICE_PASSWORD",
            "ODOO_URL",
            "CLOUDFLARE_API_TOKEN",
            "CLOUDFLARE_ZONE_ID",
        ],
        "rabbitmq.env": ["RMQ_PASS", "RABBITMQ_HOST", "RMQ_PORT", "RMQ_USER"],
        "redis.env": ["REDIS_HOST", "REDIS_PORT"],
        "smtp.env": ["SMTP_HOST", "SMTP_PORT", "SMTP_USER", "SMTP_PASS"],
        "core.env": [
            "DOMAIN",
            "SYSTEM_USER_AGENT",
            "SYSADMIN_EMAILS",
            "HAMS_CRYPTO_KEY",
            "CLOUDFLARE_TUNNEL_TOKEN",
            "PYTHONPYCACHEPREFIX",
            "WS_PORT",
            "GEMINI_API_KEY",
            "GEMINI_MODEL",
            "PLAYWRIGHT_BROWSERS_PATH",
        ],
    },
    "static_files": [
        {
            "path": "/etc/apt/sources.list.d/odoo.list",
            "content": "deb [signed-by=/usr/share/keyrings/odoo-archive-keyring.gpg] https://nightly.odoo.com/19.0/nightly/deb/ ./\n",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
        },
        {
            "path": "/opt/hams/etc/pdns_gsqlite3.conf",
            "content": """\
launch=gsqlite3
gsqlite3-database=/var/lib/powerdns/pdns.sqlite3
gsqlite3-dnssec=no
local-address=0.0.0.0
api=yes
api-key={PDNS_API_KEY}
webserver=yes
webserver-address=localhost
webserver-port=8081
webserver-allow-from=127.0.0.0/8,::1/128
dnsupdate=yes
allow-dnsupdate-from=127.0.0.0/8,::1/128
loglevel=6
""",
            "owner": "pdns:pdns",
            "mode": "640",
            "environments": ["prod"],
        },
        {
            "path": "/etc/hosts",
            "content": """\
127.0.0.1 localhost
::1 localhost ip6-localhost ip6-loopback
127.0.0.1 postgres redis rabbitmq odoo powerdns daemon_dx_firehose
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["test"],
        },
        {
            "path": "/opt/hams/systemd/hams-pycache.service",
            "content": """\
[Unit]
Description=Hams.com PyCache JIT Compiler
Before=odoo.service

[Service]
Type=oneshot
User=hams_com
Environment="PYTHONPYCACHEPREFIX=/opt/hams/pycache"
ExecStart=/opt/hams/.venv/bin/python -m compileall -q /opt/hams
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/hams.daemon.keys.service",
            "content": """\
[Unit]
Description=Hams.com Daemon Key Bootstrapper
Requires=odoo.service
After=odoo.service

[Service]
Type=oneshot
User=odoo
Environment="ODOO_RC=/opt/hams/etc/odoo.conf"
Environment="HAMS_KEYS_DIR=/opt/hams/etc/keys"
EnvironmentFile=-/opt/hams/etc/odoo.env
EnvironmentFile=-/opt/hams/etc/core.env
EnvironmentFile=-/opt/hams/etc/db.env
ExecStart=/bin/bash -c "/opt/hams/.venv/bin/python /usr/bin/odoo shell -c /opt/hams/etc/odoo.conf -d {DB_NAME} --no-http < /opt/hams/deploy/bootstrap_daemon_keys.py"
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/tmp/odoo.key",
            "url": "https://nightly.odoo.com/odoo.key",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
            "post_provision_hooks": [hook_install_odoo_key],
        },
        {
            "path": "/tmp/kopia.tar.gz",
            "url": "https://github.com/kopia/kopia/releases/download/v0.23.0/kopia-0.23.0-linux-x64.tar.gz",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
            "post_provision_hooks": [hook_install_kopia_binary],
        },
        {
            "path": "/tmp/cloudflared.deb",
            "url": "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-{DEB_TARGET_ARCH_CPU}.deb",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
            "condition_env": "CLOUDFLARE_TUNNEL_TOKEN",
            "post_provision_hooks": [hook_install_cloudflared],
        },
        {
            "path": "/tmp/PyPDF2-2.12.1.tar.gz",
            "url": "https://pypi.io/packages/source/P/PyPDF2/PyPDF2-2.12.1.tar.gz",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
            "post_provision_hooks": [hook_install_pypdf2],
        },
        {
            "src": "{REPO_ROOT}/daemons",
            "path": "/opt/hams/",
            "environments": ["prod", "test"],
        },
        {
            "src": "{REPO_ROOT}/deploy",
            "path": "/opt/hams/",
            "environments": ["prod", "test"],
        },
        {
            "src": "{REPO_ROOT}/requirements.txt",
            "path": "/opt/hams/",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/system-startup.service",
            "content": """\
[Unit]
Description=Run all timed daemons at startup
After=network.target

[Service]
Type=oneshot
ExecStart=/bin/systemctl start amsat.tle.sync.service
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/adif.processor.service",
            "content": """\
[Unit]
Description=Ham Radio ADIF Queue Processor (RabbitMQ Worker)
After=network.target rabbitmq-server.service
Requires=rabbitmq-server.service

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
ReadWritePaths=/opt/hams/spool/adif_queue
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/adif_processor

Environment="ODOO_USER=logbook_api_service_internal"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/adif_processor/adif_processor.py

Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=adif.processor

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/dx.firehose.service",
            "content": """\
[Unit]
Description=Ham Radio Ultimate DX Cluster (Live Firehose Daemon)
After=network.target postgresql.service
Requires=postgresql.service

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/dx_firehose

EnvironmentFile=/etc/hams_daemons.env
Environment="WS_PORT=8765"

LimitNOFILE=65535

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/dx_firehose/dx_firehose.py

Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=dx.firehose

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/ham.dx.daemon.service",
            "content": """\
[Unit]
Description=Ham Radio DX Cluster Telnet Daemon
After=network.target

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/ham_dx_daemon

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=dx_daemon_service"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/ham_dx_daemon/dx_daemon.py

Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=ham.dx.daemon

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/noaa-swpc-sync.service",
            "content": """\
[Unit]
Description=Ham Radio NOAA Space Weather Sync Daemon
After=network.target

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/noaa_swpc_sync

# Odoo JSON2-RPC Credentials
EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=space_weather_service"
Environment="POLL_INTERVAL=14400"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/noaa_swpc_sync/noaa_swpc_sync.py

# Resiliency
Restart=always
RestartSec=60
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/noaa.swpc.sync.service",
            "content": """\
[Unit]
Description=Ham Radio NOAA Space Weather Sync Daemon
After=network.target

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=bruce
WorkingDirectory=/home/bruce/workspace/hams_com/daemons/noaa_swpc_sync

# Odoo JSON2-RPC Credentials
EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=space_weather_service"
Environment="POLL_INTERVAL=14400"

# Execution via Python virtual environment
ExecStart=/home/bruce/workspace/hams_com/.venv/bin/python /home/bruce/workspace/hams_com/daemons/noaa_swpc_sync/noaa_swpc_sync.py

# Resiliency
Restart=always
RestartSec=60
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/pdns.sync.service",
            "content": """\
[Unit]
Description=Ham Radio PowerDNS Sync Daemon (CQRS)
After=network.target rabbitmq-server.service pdns.service
Requires=rabbitmq-server.service

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/pdns_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=dns_api_service_internal"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/pdns_sync/pdns_sync.py

Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=pdns.sync

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/lotw.eqsl.sync.service",
            "content": """\
[Unit]
Description=Ham Radio Automated QSL Sync Daemon
After=network.target

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/lotw_eqsl_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=logbook_api_service_internal"
Environment="POLL_INTERVAL=86400"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/lotw_eqsl_sync/lotw_eqsl_sync.py

Restart=always
RestartSec=60
StandardOutput=journal
StandardError=journal
SyslogIdentifier=lotw.eqsl.sync

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/amsat.tle.sync.service",
            "content": """\
[Unit]
Description=Ham Radio AMSAT TLE Sync Service
After=network.target

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=oneshot
User=odoo
WorkingDirectory=/opt/hams/daemons/amsat_tle_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=satellite_sync_service_internal"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/amsat_tle_sync/amsat_sync.py

StandardOutput=journal
StandardError=journal
SyslogIdentifier=amsat.tle.sync
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/amsat.tle.sync.timer",
            "content": """\
[Unit]
Description=Run AMSAT TLE Sync Daily

[Timer]
OnCalendar=daily
Persistent=true
RandomizedDelaySec=15m

[Install]
WantedBy=timers.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/qrz.scraper.service",
            "content": """\
[Unit]
Description=Ham Radio QRZ Scraper Daemon
After=network.target rabbitmq-server.service
Requires=rabbitmq-server.service

[Service]
# ADR-0070 OS-Level Daemon Restriction
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
PrivateDevices=true
NoNewPrivileges=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
CapabilityBoundingSet=
Type=simple
User=odoo
WorkingDirectory=/opt/hams/daemons/qrz_scraper

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=onboarding_service_internal"

# Execution via Python virtual environment
ExecStart=/opt/hams/.venv/bin/python /opt/hams/daemons/qrz_scraper/qrz_scraper.py

Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=qrz.scraper

[Install]
WantedBy=multi-user.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
    ],
    "addon_repos": [
        "hams_community",
        "hams_com",
        "hams_private_secondary",
        "hams_private_tertiary",
    ],
    "python_venvs": [
        {
            "path": "/opt/hams/.venv",
            "system_site_packages": True,
            "requirements_file": "/opt/hams/requirements.txt",
            "environments": ["prod"],
        },
        {
            "path": ".venv",
            "system_site_packages": False,
            "requirements_file": "requirements.txt",
            "environments": ["pre_flight"],
        },
    ],
    "apt_packages": [
        {"name": "postgresql", "debian_name": "postgresql", "environments": ["early_prod"]},
        {"name": "postgresql-client", "debian_name": "postgresql-client", "environments": ["early_prod"]},
        {"name": "nginx", "debian_name": "nginx", "environments": ["early_prod"]},
        {"name": "redis-server", "debian_name": "redis-server", "environments": ["early_prod"]},
        {"name": "rabbitmq-server", "debian_name": "rabbitmq-server", "environments": ["early_prod"]},
        {"name": "python3-redis", "debian_name": "python3-redis", "environments": ["early_prod"]},
        {"name": "python3-pika", "debian_name": "python3-pika", "environments": ["early_prod"]},
        {"name": "sqlite3", "debian_name": "sqlite3", "environments": ["early_prod"]},
        {"name": "pdns-server", "debian_name": "pdns-server", "environments": ["early_prod"]},
        {"name": "pdns-backend-sqlite3", "debian_name": "pdns-backend-sqlite3", "environments": ["early_prod"]},
        {"name": "pgbackrest", "debian_name": "pgbackrest", "environments": ["early_prod"]},
        {"name": "certbot", "debian_name": "certbot", "environments": ["early_prod"]},
        {"name": "python3-certbot-nginx", "debian_name": "python3-certbot-nginx", "environments": ["early_prod"]},
        {"name": "python3-venv", "debian_name": "python3-venv", "environments": ["early_prod"]},
        {"name": "python3-passlib", "debian_name": "python3-passlib", "environments": ["early_prod"]},
        {"name": "python3-cryptography", "debian_name": "python3-cryptography", "environments": ["early_prod"]},
        {"name": "build-essential", "debian_name": "build-essential", "environments": ["early_prod"]},
        {"name": "libpq-dev", "debian_name": "libpq-dev", "environments": ["early_prod"]},
        {"name": "python3-dev", "debian_name": "python3-dev", "environments": ["early_prod"]},
        {"name": "bind9-dnsutils", "debian_name": "dnsutils", "environments": ["early_prod"]},
        {"name": "python3-stdeb", "debian_name": "python3-stdeb", "environments": ["early_prod"]},
        {"name": "fakeroot", "debian_name": "fakeroot", "environments": ["early_prod"]},
        {"name": "python3-all", "debian_name": "python3-all", "environments": ["early_prod"]},
        {"name": "python3-setuptools", "debian_name": "python3-setuptools", "environments": ["early_prod"]},
        {"name": "dh-python", "debian_name": "dh-python", "environments": ["early_prod"]},
        {"name": "jing", "debian_name": "jing", "environments": ["early_prod"]},
    ],
    "env_defaults": {
        "DB_PORT": "5432",
        "RMQ_PORT": "5672",
        "REDIS_PORT": "6379",
        "WS_PORT": "8765",
        "RMQ_USER": "guest",
        "RMQ_PASS": "guest",
        "PLAYWRIGHT_BROWSERS_PATH": "/opt/hams/cache/ms-playwright",
    },
    "systemd_odoo_override": {
        "Unit": {"Requires": "hams-pycache.service", "After": "hams-pycache.service"},
        "Service": {
            "EnvironmentFiles": [
                "odoo.env",
                "core.env",
                "db.env",
                "redis.env",
                "rabbitmq.env",
                "smtp.env",
                "pdns.env",
            ],
            "Environment": ["PYTHONPYCACHEPREFIX=/opt/hams/pycache"],
            "ProtectSystem": "strict",
            "BindPaths": "/opt/hams/etc/keys:/var/lib/odoo/daemon_keys",
            "PrivateTmp": "true",
            "PrivateDevices": "true",
            "NoNewPrivileges": "true",
            "KillSignal": "SIGINT",
            "TimeoutStopSec": "15",
        },
    },
}


def scaffold_test_environment(args_db, provision_dirs=True):
    for k, v in MANIFEST["env_defaults"].items():
        os.environ.setdefault(k, v)

    os.environ.setdefault("DB_NAME", args_db)
    os.environ.setdefault("ODOO_DB", args_db)
    os.environ.setdefault("DB_USER", "odoo")
    os.environ.setdefault("DB_PASS", "odoo")
    os.environ.setdefault("DB_HOST", "postgres")
    os.environ.setdefault("ODOO_URL", "http://odoo:8069")
    os.environ.setdefault("PDNS_API_URL", "http://powerdns:8081/api/v1/servers/localhost/zones")
    os.environ.setdefault("PDNS_API_KEY", "secret")

    if provision_dirs:
        try:
            for d in MANIFEST["directories"]:
                if "test" in d["environments"]:
                    os.makedirs(d["path"], exist_ok=True)
                    mode = int(d["provision_mode"], 8)
                    apply_permissions(d["path"], d.get("owner"), mode, recursive=True)
        except PermissionError:
            print("[*] Elevating briefly to provision required host directories...")
            for d in MANIFEST["directories"]:
                if "test" in d["environments"]:
                    path = d["path"]
                    mode_str = d["provision_mode"]
                    subprocess.run(["sudo", "mkdir", "-p", path], check=True)
                    subprocess.run(["sudo", "chmod", "-R", mode_str, path], check=True)
                    if d.get("owner"):
                        subprocess.run(["sudo", "chown", "-R", d["owner"], path], check=True)


def get_mount_paths(environment, mount_type):
    return [d["path"] for d in MANIFEST["directories"] if environment in d["environments"] and d.get("runtime_mount") == mount_type]


def provision_system_accounts(run_cmd_func, environment="prod", dest_dir=""):
    for acc in MANIFEST.get("system_accounts", []):
        if environment not in acc.get("environments", ["prod", "test"]):
            continue

        user = acc["user"]
        group = acc["group"]
        home = acc.get("home", "/opt/hams")
        shell = acc.get("shell", "/bin/bash")
        add_to_users = acc.get("add_to_users", [])

        try:
            grp.getgrnam(group)
        except KeyError:
            run_cmd_func(["groupadd", "--system", group])

        try:
            pwd.getpwnam(user)
        except KeyError:
            run_cmd_func(["useradd", "--system", "-g", group, "-d", home, "-s", shell, user])

        for extra_user in add_to_users:
            try:
                pwd.getpwnam(extra_user)
                run_cmd_func(["usermod", "-a", "-G", group, extra_user])
            except KeyError:
                _logger.debug("User %s not found, skipping group addition.", extra_user)


def execute_hooks(environment, run_cmd_func, env_vars=None, dest_dir=""):
    if dest_dir and dest_dir.endswith("/"):
        dest_dir = dest_dir[:-1]

    for d in MANIFEST["directories"]:
        if environment in d["environments"] and "post_provision_hooks" in d:
            for hook in d["post_provision_hooks"]:
                hook(env_vars or {}, dest_dir, d["path"], run_cmd_func)


def apply_production_directories(run_cmd_func, environment="prod", dest_dir=""):
    provision_system_accounts(run_cmd_func, environment, dest_dir)
    for d in MANIFEST["directories"]:
        if environment in d["environments"]:
            path = os.path.join(dest_dir, d["path"].lstrip("/")) if dest_dir else d["path"]
            mode = int(d["provision_mode"], 8)
            os.makedirs(path, mode=mode, exist_ok=True)
            apply_permissions(path, d.get("owner"), mode, recursive=True)


def write_env_files(base_etc_dir, env_vars, run_cmd_func, dest_dir=""):
    if dest_dir:
        base_etc_dir = os.path.join(dest_dir, base_etc_dir.lstrip("/"))
    os.makedirs(base_etc_dir, exist_ok=True)

    for filename, keys in MANIFEST["env_groups"].items():
        filepath = os.path.join(base_etc_dir, filename)
        content = "".join(f"{k}={env_vars[k]}\n" for k in keys if k in env_vars)

        flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
        fd = os.open(filepath, flags, 0o400)
        with open(fd, "w", encoding="utf-8") as f:
            f.write(content)

        apply_permissions(filepath, "root:root", 0o400)


def provision_apt_packages(run_cmd_func, environment="prod"):
    os_id = get_os_identifier()
    packages = []
    for p in MANIFEST.get("apt_packages", []):
        if environment in p["environments"]:
            pkg_name = p.get("debian_name", p["name"]) if os_id == "debian" else p["name"]
            packages.append(pkg_name)
    if packages:
        run_cmd_func(["apt-get", "update"])
        run_cmd_func(["apt-get", "install", "-y"] + packages)


def provision_custom_addons(run_cmd_func, env_vars, environment="prod", dest_dir=""):
    if environment not in ["prod", "test"]:
        return

    repos = MANIFEST.get("addon_repos", [])
    repo_root = env_vars.get("REPO_ROOT")
    if not repos or not repo_root:
        return

    base_dir = os.path.abspath(os.path.join(repo_root, ".."))
    custom_addons_dir = os.path.join(dest_dir, "opt/hams/odoo") if dest_dir else "/opt/hams/odoo"

    for repo_name in repos:
        repo_path = repo_root if repo_name == os.path.basename(repo_root) else os.path.join(base_dir, repo_name)
        if os.path.isdir(repo_path):
            for item in os.listdir(repo_path):
                item_path = os.path.join(repo_path, item)
                if os.path.isdir(item_path) and os.path.exists(os.path.join(item_path, "__manifest__.py")):
                    target = os.path.join(custom_addons_dir, item)
                    shutil.rmtree(target, ignore_errors=True)
                    os.makedirs(target, exist_ok=True)
                    shutil.copytree(item_path, target, dirs_exist_ok=True)

    apply_permissions(custom_addons_dir, "odoo:odoo", None, recursive=True)


def provision_python_venvs(run_cmd_func, environment="prod", dest_dir=""):
    for venv_spec in MANIFEST.get("python_venvs", []):
        if environment not in venv_spec["environments"]:
            continue

        venv_path = venv_spec["path"]
        req_file = venv_spec.get("requirements_file")

        if dest_dir:
            venv_path = os.path.join(dest_dir, venv_path.lstrip("/"))
            if req_file:
                req_file = os.path.join(dest_dir, req_file.lstrip("/"))

        if not os.path.exists(venv_path):
            cmd = ["/usr/bin/python3", "-m", "venv"]
            if venv_spec.get("system_site_packages"):
                cmd.append("--system-site-packages")
            cmd.append(venv_path)
            run_cmd_func(cmd)

        pip_exe = os.path.join(venv_path, "bin", "pip")
        if req_file:
            if not os.path.exists(req_file):
                raise FileNotFoundError(f"Required requirements file not found: {req_file}")
            run_cmd_func([pip_exe, "install", "-r", req_file])

            playwright_exe = os.path.join(venv_path, "bin", "playwright")
            if os.path.exists(playwright_exe):
                run_cmd_func(["env", "PLAYWRIGHT_BROWSERS_PATH=/opt/hams/cache/ms-playwright", playwright_exe, "install", "chromium"])


def provision_static_files(run_cmd_func, env_vars, environment="prod", dest_dir=""):
    for file_spec in MANIFEST.get("static_files", []):
        if environment not in file_spec["environments"]:
            continue

        condition_env = file_spec.get("condition_env")
        if condition_env and not env_vars.get(condition_env):
            continue

        path = format_env(file_spec["path"], env_vars)
        if dest_dir:
            path = os.path.join(dest_dir, path.lstrip("/"))

        os.makedirs(os.path.dirname(path), exist_ok=True)
        mode = int(file_spec.get("mode", "644"), 8)

        src = file_spec.get("src")
        url = file_spec.get("url")

        if src:
            src = format_env(src, env_vars)
            if os.path.exists(src):
                if os.path.isdir(src):
                    shutil.copytree(src, path, dirs_exist_ok=True)
                else:
                    shutil.copy2(src, path)
        elif url:
            if "{DEB_TARGET_ARCH_CPU}" in url and "DEB_TARGET_ARCH_CPU" not in env_vars:
                res = subprocess.run(["dpkg-architecture", "-q", "DEB_TARGET_ARCH_CPU"], capture_output=True, text=True)
                if res.returncode == 0:
                    env_vars["DEB_TARGET_ARCH_CPU"] = res.stdout.strip()
            download_file(format_env(url, env_vars), path, mode, env_vars)
        else:
            content = format_env(file_spec.get("content", ""), env_vars)
            flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
            fd = os.open(path, flags, mode)
            with open(fd, "w", encoding="utf-8") as f:
                f.write(content)

        apply_permissions(path, file_spec.get("owner"), mode, recursive=os.path.isdir(path))

        if "post_provision_hooks" in file_spec:
            for hook in file_spec["post_provision_hooks"]:
                hook(env_vars or {}, dest_dir, path, run_cmd_func)


def generate_odoo_override_conf(odoo_conf_path):
    spec = MANIFEST["systemd_odoo_override"]
    lines = ["[Unit]"] + [f"{k}={v}" for k, v in spec["Unit"].items()] + ["", "[Service]", f'Environment="ODOO_RC={odoo_conf_path}"']
    if "Group" in spec["Service"]:
        lines.append(f"Group={spec['Service']['Group']}")
    lines.extend([f"EnvironmentFile=-/opt/hams/etc/{e}" for e in spec["Service"]["EnvironmentFiles"]])
    lines.extend([f'Environment="{e}"' for e in spec["Service"]["Environment"]])
    lines.append(f"ProtectSystem={spec['Service']['ProtectSystem']}")
    lines.append(f"ReadWritePaths=/var/lib/odoo /var/log/odoo {' '.join(get_mount_paths('prod', 'rw'))}")
    lines.append(f"ReadOnlyPaths={' '.join(get_mount_paths('prod', 'ro'))}")

    for key in ["BindPaths", "PrivateTmp", "PrivateDevices", "NoNewPrivileges", "KillSignal", "TimeoutStopSec"]:
        if key in spec["Service"]:
            lines.append(f"{key}={spec['Service'][key]}")

    lines.append("ExecStart=")
    lines.append(f"ExecStart=/opt/hams/.venv/bin/python /usr/bin/odoo -c {odoo_conf_path}")
    return "\n".join(lines) + "\n"


def provision_jules_environment(run_cmd_func, env_vars, base_dir, orig_user):
    try:
        with open("/etc/hosts", "r") as f:
            hosts_content = f.read()
        if "redis" not in hosts_content:
            _logger.info("[*] Ensuring docker-compose hostnames resolve locally in /etc/hosts...")
            with open("/etc/hosts", "a") as f:
                f.write("\n127.0.1.1 redis rabbitmq postgres pdns memcached\n")
    except OSError as e:
        _logger.warning("[*] Failed to update /etc/hosts: %s", e)

    is_hams_community = os.path.exists(os.path.join(base_dir, "zero_sudo", "__manifest__.py"))
    if not is_hams_community:
        target_clone = "/hams_community"
        if not os.path.exists(target_clone):
            _logger.info("[*] Sibling repository not found. Cloning hams_community to %s...", target_clone)
            try:
                clone_env = dict(env_vars)
                clone_env["GIT_TERMINAL_PROMPT"] = "0"
                run_cmd_func(["git", "clone", "https://github.com/BrucePerens/hams_community", target_clone], env=clone_env)
                if orig_user:
                    try:
                        u_info = pwd.getpwnam(orig_user)
                        run_cmd_func(["chown", "-R", f"{u_info.pw_uid}:{u_info.pw_gid}", target_clone])
                    except KeyError as e:
                        _logger.debug("Original user %s not found: %s", orig_user, e)
            except subprocess.CalledProcessError as e:
                _logger.warning("[*] Failed to clone to %s: %s", target_clone, e)
                _logger.error("[!] DIAGNOSTIC FOR AI: The hams_community repository could not be cloned due to GitHub authentication restrictions in this headless VM.")
                _logger.error("    If the required 'zero_sudo' module is not present in your current working directory, the tests will fatally crash.")
                _logger.error("    ACTION: Document this blocker in JULES_ISSUES.md and gracefully conclude your session.")

    _logger.info("[*] Provisioning APT Sources and Packages...")
    try:
        apt_opts = ["-o", "Dpkg::Options::=--force-confdef", "-o", "Dpkg::Options::=--force-confold", "-o", "Dpkg::Lock::Timeout=120"]

        run_cmd_func(["apt-get", "update"] + apt_opts)
        run_cmd_func(["apt-get", "install", "-y"] + apt_opts + ["gnupg", "lsb-release"])
        run_cmd_func(["bash", "-c", "curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor --yes -o /usr/share/keyrings/postgresql-keyring.gpg"])
        run_cmd_func(["bash", "-c", "echo \"deb [signed-by=/usr/share/keyrings/postgresql-keyring.gpg] http://apt.postgresql.org/pub/repos/apt/ $(lsb_release -cs)-pgdg main\" > /etc/apt/sources.list.d/pgdg.list"])

        provision_system_accounts(run_cmd_func, environment="prod")
        provision_static_files(run_cmd_func, env_vars, environment="prod")
        run_cmd_func(["apt-get", "update"] + apt_opts + ["--allow-insecure-repositories"])

        all_packages = [
            "python3-setuptools", "python3-stdeb", "dh-python", "python3-all",
            "fakeroot", "postgresql-common", "postgresql-client",
            "postgresql", "odoo"
        ]

        os_id = get_os_identifier()
        jules_provided = {"curl", "python3-pip", "build-essential"}
        for pkg_spec in MANIFEST.get("apt_packages", []):
            if "early_prod" in pkg_spec["environments"]:
                pkg_name = pkg_spec.get("debian_name", pkg_spec["name"]) if os_id == "debian" else pkg_spec["name"]
                if pkg_spec["name"] not in jules_provided and pkg_name not in jules_provided:
                    all_packages.append(pkg_name)

        pg_res = subprocess.run(
            ["bash", "-c", "apt-cache depends postgresql | grep -Eo 'postgresql-[0-9]+' | head -n1 | grep -Eo '[0-9]+'"],
            capture_output=True, text=True
        )
        if pg_res.returncode == 0 and pg_res.stdout.strip():
            pg_major = pg_res.stdout.strip()
            all_packages.append(f"postgresql-{pg_major}-pgvector")

        all_packages = sorted(list(set(all_packages)))
        run_cmd_func(["apt-get", "install", "-y"] + apt_opts + all_packages)

        req_file = os.path.join(base_dir, "requirements.txt")
        if os.path.exists(req_file):
            try:
                pip_env = dict(env_vars)
                pip_env["PIP_ROOT_USER_ACTION"] = "ignore"
                run_cmd_func(["/usr/bin/python3", "-m", "pip", "install", "--break-system-packages", "--ignore-installed", "-r", req_file], env=pip_env)
            except subprocess.CalledProcessError as e:
                _logger.warning("[*] pip install encountered an error: %s", e)

        _logger.info("[*] Preparing testing directories with production paths...")
        apply_production_directories(run_cmd_func, environment="prod")
        apply_production_directories(run_cmd_func, environment="test")

        if orig_user:
            try:
                u_info = pwd.getpwnam(orig_user)
                user_tmp = os.path.join(u_info.pw_dir, "tmp")
                os.makedirs(user_tmp, exist_ok=True)
                apply_permissions(user_tmp, f"{orig_user}:{orig_user}", None)

            except KeyError as e:
                _logger.debug("Original user %s not found: %s", orig_user, e)

    except subprocess.CalledProcessError as e:
        _logger.error("Failed to provision system packages: %s", e)
        sys.exit(1)
