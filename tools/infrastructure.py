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
import time
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

def get_os_codename():
    try:
        with open("/etc/os-release") as f:
            for line in f:
                if line.startswith("VERSION_CODENAME="):
                    return line.strip().split("=")[1].strip('"').lower()
    except OSError as e:
        _logger.debug("Ignored OSError reading /etc/os-release: %s", e)
    return "jammy"

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

def apply_permissions(path, owner_str, mode_int):
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


def hook_install_pg_key(env_vars, dest_dir, path, run_cmd_func):
    out = os.path.join(dest_dir, 'usr/share/keyrings/postgresql-keyring.gpg') if dest_dir else '/usr/share/keyrings/postgresql-keyring.gpg'
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
            "path": "/etc/odoo",
            "owner": "odoo:odoo",
            "provision_mode": "750",
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
ExecStart=/usr/bin/python3 -m compileall -q /opt/hams
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
Environment="ODOO_RC=/etc/odoo/odoo.conf"
Environment="HAMS_KEYS_DIR=/opt/hams/etc/keys"
EnvironmentFile=-/opt/hams/etc/odoo.env
EnvironmentFile=-/opt/hams/etc/core.env
EnvironmentFile=-/opt/hams/etc/db.env
ExecStart=/bin/bash -c "/usr/bin/python3 /usr/bin/odoo shell -c /etc/odoo/odoo.conf -d {DB_NAME} --no-http < /opt/hams/deploy/bootstrap_daemon_keys.py"
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
            "path": "/tmp/pg.key",
            "url": "https://www.postgresql.org/media/keys/ACCC4CF8.asc",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
            "post_provision_hooks": [hook_install_pg_key],
        },
        {
            "path": "/etc/apt/sources.list.d/pgdg.list",
            "content": "deb [signed-by=/usr/share/keyrings/postgresql-keyring.gpg] http://apt.postgresql.org/pub/repos/apt/ {DEB_CODENAME}-pgdg main\n",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/adif_processor/adif_processor.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/dx_firehose/dx_firehose.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/ham_dx_daemon/dx_daemon.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/noaa_swpc_sync/noaa_swpc_sync.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /home/bruce/workspace/hams_com/daemons/noaa_swpc_sync/noaa_swpc_sync.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/pdns_sync/pdns_sync.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/lotw_eqsl_sync/lotw_eqsl_sync.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/amsat_tle_sync/amsat_sync.py

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

# Execution via system Python
ExecStart=/usr/bin/python3 /opt/hams/daemons/qrz_scraper/qrz_scraper.py

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
    "apt_packages": [
        {"name": "odoo", "debian_name": "odoo", "environments": ["early_prod"]},
        {"name": "postgresql", "debian_name": "postgresql", "environments": ["early_prod"]},
        {"name": "postgresql-common", "debian_name": "postgresql-common", "environments": ["early_prod"]},
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
        # Debian deprecated pypdf2 and moved all development to the pypdf package.
        # We might have to create a bridge package to satisfy the python3-pypdf2
        # dependency, if we install the Ubuntu odoo package on Debian.
        {"name": "python3-pypdf2", "debian_name": "python3-pypdf", "environments": ["early_prod"]},
        {"name": "python3-setuptools", "debian_name": "python3-setuptools", "environments": ["early_prod"]},
        {"name": "dh-python", "debian_name": "dh-python", "environments": ["early_prod"]},
        {"name": "jing", "debian_name": "jing", "environments": ["early_prod"]},
        {"name": "dbus-x11", "debian_name": "dbus-x11", "environments": ["early_prod"]},
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
            "EnvironmentFile": [
                "-/opt/hams/etc/odoo.env",
                "-/opt/hams/etc/core.env",
                "-/opt/hams/etc/db.env",
                "-/opt/hams/etc/redis.env",
                "-/opt/hams/etc/rabbitmq.env",
                "-/opt/hams/etc/smtp.env",
                "-/opt/hams/etc/pdns.env",
            ],
            "Environment": [
                "PYTHONPYCACHEPREFIX=/opt/hams/pycache",
                "ODOO_RC=/etc/odoo/odoo.conf"
            ],
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
            apply_production_directories(environment="test")
        except PermissionError as e:
            print(f"[*] PermissionError provisioning test directories: {e}")
            print("[*] Note: 'sudo' fallback removed per strict DevSecOps mandates.")
            raise


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


def apply_production_directories(run_cmd_func=None, environment="prod", dest_dir=""):
    for d in MANIFEST["directories"]:
        if environment in d["environments"]:
            path = os.path.join(dest_dir, d["path"].lstrip("/")) if dest_dir else d["path"]
            mode = int(d["provision_mode"], 8)
            os.makedirs(path, mode=mode, exist_ok=True)
            apply_permissions(path, d.get("owner"), mode)


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


def provision_custom_addons(run_cmd_func, env_vars, environment="prod", dest_dir=""):
    if environment not in ["prod", "test"]:
        return

    if not env_vars.get("REPO_ROOT"):
        return

    custom_addons_dir = os.path.join(dest_dir, "opt/hams/odoo") if dest_dir else "/opt/hams/odoo"

    if os.path.isdir(env_vars["REPO_ROOT"]):
        for item in os.listdir(env_vars["REPO_ROOT"]):
            item_path = os.path.join(env_vars["REPO_ROOT"], item)
            if os.path.isdir(item_path) and os.path.exists(os.path.join(item_path, "__manifest__.py")):
                target = os.path.join(custom_addons_dir, item)
                shutil.rmtree(target, ignore_errors=True)
                os.makedirs(target, exist_ok=True)
                shutil.copytree(item_path, target, dirs_exist_ok=True)

    apply_permissions(custom_addons_dir, "odoo:odoo", None)


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
            if "{DEB_CODENAME}" in file_spec.get("content", "") and "DEB_CODENAME" not in env_vars:
                env_vars["DEB_CODENAME"] = get_os_codename()
            content = format_env(file_spec.get("content", ""), env_vars)
            flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
            fd = os.open(path, flags, mode)
            with open(fd, "w", encoding="utf-8") as f:
                f.write(content)

        apply_permissions(path, file_spec.get("owner"), mode)

        if "post_provision_hooks" in file_spec:
            for hook in file_spec["post_provision_hooks"]:
                hook(env_vars or {}, dest_dir, path, run_cmd_func)

def provision_systemd_override(run_cmd_func, env_vars, environment="prod", dest_dir=""):
    if environment not in ["prod", "test"]:
        return
    override_data = MANIFEST.get("systemd_odoo_override")
    if not override_data:
        return

    override_dir = os.path.join(dest_dir, "etc/systemd/system/odoo.service.d".lstrip("/")) if dest_dir else "/etc/systemd/system/odoo.service.d"
    os.makedirs(override_dir, exist_ok=True)
    override_file = os.path.join(override_dir, "override.conf")

    lines = []
    for section, items in override_data.items():
        lines.append(f"[{section}]")
        for k, v in items.items():
            if isinstance(v, list):
                for item in v:
                    lines.append(f"{k}={item}")
            else:
                lines.append(f"{k}={v}")
        lines.append("")

    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
    fd = os.open(override_file, flags, 0o644)
    with open(fd, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))

    apply_permissions(override_file, "root:root", 0o644)

    if not dest_dir and run_cmd_func:
        try:
            run_cmd_func(["systemctl", "daemon-reload"])
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Failed to reload systemd daemons: %s", e)

def run_post_provision_smoketest():
    _logger.info("[*] Running post-provisioning smoketest on all services...")

    try:
        subprocess.run(["systemctl", "daemon-reload"], check=False)
    except OSError as e:
        _logger.debug("Ignored OSError during daemon-reload: %s", e)

    potential_services = [
        "postgresql", "redis-server", "rabbitmq-server", "pdns", "odoo"
    ]

    for sf in MANIFEST.get("static_files", []):
        path = sf.get("path", "")
        if "systemd" in path and path.endswith(".service"):
            svc_name = os.path.basename(path)
            if svc_name not in potential_services and "@" not in svc_name:
                potential_services.append(svc_name)

    services_to_test = []
    for svc in potential_services:
        res = subprocess.run(["systemctl", "status", svc], capture_output=True, text=True)
        if "could not be found" not in res.stderr and "could not be found" not in res.stdout:
            services_to_test.append(svc)

    started_services = []

    for svc in services_to_test:
        _logger.info("    Starting %s...", svc)
        res = subprocess.run(["systemctl", "start", svc], capture_output=True, text=True)
        started_services.append(svc)
        if res.returncode != 0:
            _logger.error("    [!] systemctl start %s returned non-zero exit code: %s", svc, res.returncode)
            _logger.error("stdout: %s", res.stdout)
            _logger.error("stderr: %s", res.stderr)
            logs = subprocess.run(["journalctl", "-u", svc, "-n", "100", "--no-pager"], capture_output=True, text=True)
            _logger.error("--- LOGS FOR %s ---\n%s\n-------------------", svc, logs.stdout)
            sys.exit(1)

    _logger.info("[*] Waiting for services to stabilize (10 seconds)...")
    time.sleep(10)

    failed = False
    for svc in started_services:
        res = subprocess.run(["systemctl", "is-failed", svc], capture_output=True, text=True)
        state = res.stdout.strip()
        if state == "failed":
            _logger.error("[!] Service %s failed to start or crashed.", svc)
            logs = subprocess.run(["journalctl", "-u", svc, "-n", "100", "--no-pager"], capture_output=True, text=True)
            _logger.error("--- LOGS FOR %s ---\n%s\n-------------------", svc, logs.stdout)
            failed = True

    if failed:
        _logger.error("[!] One or more services failed the smoketest. Aborting snapshot.")
        sys.exit(1)

    _logger.info("[*] All services started successfully. Shutting them down...")
    for svc in reversed(started_services):
        _logger.info("    Stopping %s...", svc)
        subprocess.run(["systemctl", "stop", svc], capture_output=True)

    _logger.info("[*] Smoketest complete.")

def provision_jules_environment(run_cmd_func, env_vars, orig_user):
    try:
        with open("/etc/hosts", "r") as f:
            hosts_content = f.read()
        if "redis" not in hosts_content:
            _logger.info("[*] Ensuring docker-compose hostnames resolve locally in /etc/hosts...")
            with open("/etc/hosts", "a") as f:
                f.write("\n127.0.1.1 redis rabbitmq postgres pdns memcached\n")
    except OSError as e:
        _logger.warning("[*] Failed to update /etc/hosts: %s", e)

    is_hams_community = os.path.exists(os.path.join(env_vars.get("REPO_ROOT", "/app"), "zero_sudo", "__manifest__.py"))
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
        run_cmd_func(["apt-get", "install", "-y"] + apt_opts + ["gnupg"])

        provision_system_accounts(run_cmd_func, environment="prod")
        provision_static_files(run_cmd_func, env_vars, environment="prod")
        provision_systemd_override(run_cmd_func, env_vars, environment="prod")
        run_cmd_func(["apt-get", "update"] + apt_opts + ["--allow-insecure-repositories"])

        all_packages = []

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

        try:
            run_cmd_func(["bash", "-c", "sed -i 's/peer/trust/g; s/md5/trust/g; s/scram-sha-256/trust/g' /etc/postgresql/*/main/pg_hba.conf"])
            run_cmd_func(["bash", "-c", "echo \"shared_preload_libraries = 'pg_stat_statements'\" >> /etc/postgresql/*/main/postgresql.conf"])
            run_cmd_func(["systemctl", "restart", "postgresql"])
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("[*] Failed to configure PostgreSQL settings: %s", e)

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

        _logger.info("[*] Linking custom systemd units...")
        try:
            systemd_dir = "/opt/hams/systemd"
            if os.path.exists(systemd_dir):
                for item in os.listdir(systemd_dir):
                    if item.endswith(".service") or item.endswith(".timer"):
                        src = os.path.join(systemd_dir, item)
                        dst = os.path.join("/etc/systemd/system", item)
                        if not os.path.exists(dst):
                            os.symlink(src, dst)
        except OSError as e:
            _logger.warning("Failed to link systemd units: %s", e)

        run_post_provision_smoketest()

    except subprocess.CalledProcessError as e:
        _logger.error("Failed to provision system packages: %s", e)
        sys.exit(1)
