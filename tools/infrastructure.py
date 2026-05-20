#!/usr/bin/env python3
"""
Infrastructure Blueprint & Provisioning Engine
Serves as the Single Source of Truth for test_runner.py and deploy_wizard.py.
Supports environment scoping, lifecycle hooks, and precise runtime mount states.
"""

import os
import subprocess
import urllib.request

MANIFEST = {
    "directories": [
        {
            "path": "/opt/hams",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/etc",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/etc/keys",
            "owner": "odoo:odoo",
            "provision_mode": "700",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/nginx",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod"],
        },
        {
            "path": "/opt/hams/nginx/ssl",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod"],
            "post_provision_hooks": [
                """\
                if [ ! -f {DEST_DIR}/opt/hams/nginx/ssl/fullchain.pem ]; then
                    openssl req -x509 -nodes -days 3650 -newkey rsa:2048 \\
                        -keyout {DEST_DIR}/opt/hams/nginx/ssl/privkey.pem \\
                        -out {DEST_DIR}/opt/hams/nginx/ssl/fullchain.pem \\
                        -subj /C=US/ST=CA/L=SF/O=Hams/CN={DOMAIN} 2>/dev/null
                    cp {DEST_DIR}/opt/hams/nginx/ssl/fullchain.pem {DEST_DIR}/opt/hams/nginx/ssl/lotw_root.pem
                fi\
                """
            ],
        },
        {
            "path": "/deploy/ssl",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["docker"],
            "post_provision_hooks": [
                """\
                if [ ! -f {DEST_DIR}/deploy/ssl/fullchain.pem ]; then
                    openssl req -x509 -nodes -days 3650 -newkey rsa:2048 \\
                        -keyout {DEST_DIR}/deploy/ssl/privkey.pem \\
                        -out {DEST_DIR}/deploy/ssl/fullchain.pem \\
                        -subj /C=US/ST=CA/L=SF/O=Hams/CN={DOMAIN} 2>/dev/null
                    cp {DEST_DIR}/deploy/ssl/fullchain.pem {DEST_DIR}/deploy/ssl/lotw_root.pem
                fi\
                """
            ],
        },
        {
            "path": "/opt/hams/odoo",
            "owner": "odoo:odoo",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/cache",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/cache/ms-playwright",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/pycache",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
            "post_provision_hooks": [
                "rm -rf {DEST_DIR}/opt/hams/pycache/*",
                """\
                if [ -d {DEST_DIR}/opt/hams/daemons ]; then
                    python3 -m compileall -q {DEST_DIR}/opt/hams/daemons 2>/dev/null || true
                fi\
                """,
            ],
        },
        {
            "path": "/opt/hams/spool",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/spool/adif_queue",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/spool/ncvec",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/pgdata",
            "owner": "postgres:postgres",
            "provision_mode": "700",
            "runtime_mount": "rw",
            "environments": ["test"],
        },
        {
            "path": "/opt/hams/pgsock",
            "owner": "postgres:postgres",
            "provision_mode": "777",
            "runtime_mount": "rw",
            "environments": ["test"],
        },
        {
            "path": "/opt/hams/failed_input",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/downloads",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/test",
            "owner": "odoo:odoo",
            "provision_mode": "777",
            "runtime_mount": "rw",
            "environments": ["test"],
        },
        {
            "path": "/var/lib/odoo",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local/share",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local/share/Odoo",
            "owner": "odoo:odoo",
            "provision_mode": "775",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/odoo/.local/share/Odoo/sessions",
            "owner": "odoo:odoo",
            "provision_mode": "700",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/rabbitmq",
            "owner": "rabbitmq:rabbitmq",
            "provision_mode": "755",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/log/odoo",
            "owner": "odoo:odoo",
            "provision_mode": "755",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/powerdns",
            "owner": "pdns:pdns",
            "provision_mode": "755",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/odoo.service.d",
            "owner": "root:root",
            "provision_mode": "755",
            "runtime_mount": "ro",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/log/redis",
            "owner": "redis:redis",
            "provision_mode": "755",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/log/rabbitmq",
            "owner": "rabbitmq:rabbitmq",
            "provision_mode": "755",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/var/lib/redis",
            "owner": "redis:redis",
            "provision_mode": "755",
            "runtime_mount": "rw",
            "environments": ["prod", "test"],
        },
        {
            "path": "/tmp/odoo_test_home",
            "owner": "root:root",
            "provision_mode": "777",
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
            "path": "/etc/apt/sources.list.d/kopia.list",
            "content": "deb [signed-by=/usr/share/keyrings/kopia-keyring.gpg] http://packages.kopia.io/apt/ stable main\n",
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
User=root
Group=root
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
Group=odoo
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
            "post_provision_hooks": [
                "gpg --dearmor -o {DEST_DIR}/usr/share/keyrings/odoo-archive-keyring.gpg --yes {PATH}"
            ],
        },
        {
            "path": "/tmp/kopia-keyring.key",
            "url": "https://kopia.io/signing-key",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
            "post_provision_hooks": [
                "gpg --dearmor -o {DEST_DIR}/usr/share/keyrings/kopia-keyring.gpg --yes {PATH}"
            ],
        },
        {
            "path": "/tmp/cloudflared.deb",
            "url": "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-{DEB_TARGET_ARCH_CPU}.deb",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod"],
            "condition_env": "CLOUDFLARE_TUNNEL_TOKEN",
            "post_provision_hooks": [
                "dpkg -i {PATH} || true",
                "cloudflared service install {CLOUDFLARE_TUNNEL_TOKEN} || true",
                "rm -f {PATH}",
            ],
        },
        {
            "path": "/tmp/PyPDF2-2.12.1.tar.gz",
            "url": "https://pypi.io/packages/source/P/PyPDF2/PyPDF2-2.12.1.tar.gz",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
            "post_provision_hooks": [
                "if [ -s {PATH} ]; then tar -xzf {PATH} -C {DEST_DIR}/tmp || tar -xzf {DEST_DIR}/{PATH} -C {DEST_DIR}/tmp; fi || true",
                "if [ -d {DEST_DIR}/tmp/PyPDF2-2.12.1 ]; then echo \"from setuptools import setup, find_packages\\nsetup(name='PyPDF2', version='2.12.1', packages=find_packages(), include_package_data=True, description='A pure-python PDF library')\" > {DEST_DIR}/tmp/PyPDF2-2.12.1/setup.py; fi",
                "if [ -d {DEST_DIR}/tmp/PyPDF2-2.12.1 ]; then echo -e '[DEFAULT]\\nX-Python3-Version: >= 3.6' > {DEST_DIR}/tmp/PyPDF2-2.12.1/stdeb.cfg; fi",
                "if [ -d {DEST_DIR}/tmp/PyPDF2-2.12.1 ]; then cd {DEST_DIR}/tmp/PyPDF2-2.12.1 && python3 setup.py --command-packages=stdeb.command bdist_deb; fi",
                "if [ -f {DEST_DIR}/tmp/PyPDF2-2.12.1/deb_dist/python3-pypdf2_2.12.1-1_all.deb ]; then dpkg -i {DEST_DIR}/tmp/PyPDF2-2.12.1/deb_dist/python3-pypdf2_2.12.1-1_all.deb || true; fi",
                "rm -rf {DEST_DIR}/tmp/PyPDF2-2.12.1 {PATH} 2>/dev/null || true",
            ],
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
ExecStart=/bin/systemctl start uk.ofcom.sync.service nz.rsm.sync.service de.bnetza.sync.service br.anatel.sync.service au.acma.sync.service amsat.tle.sync.service
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
Group=odoo
WorkingDirectory=/opt/hams/daemons/adif_processor

Environment="ODOO_USER=logbook_api_service_internal"

ExecStart=/usr/bin/env bash /opt/hams/daemons/run_daemon.sh /opt/hams/daemons/adif_processor adif_processor.py

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
            "path": "/opt/hams/systemd/br.anatel.sync.service",
            "content": """\
[Unit]
Description=Ham Radio Brazil Callbook Sync Service
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/br_anatel_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"
Environment="BR_MIRRORS=https://data.hamradiodata.example/br/callbook.csv,https://mirror2.example.com/br_callsigns.csv"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/br_anatel_sync br_sync.py

StandardOutput=journal
StandardError=journal
SyslogIdentifier=br.anatel.sync
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/br.anatel.sync.timer",
            "content": """\
[Unit]
Description=Run Brazil Callbook Sync Hourly

[Timer]
OnCalendar=hourly
Persistent=true
RandomizedDelaySec=5m

[Install]
WantedBy=timers.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/de.bnetza.sync.service",
            "content": """\
[Unit]
Description=Ham Radio Germany Callbook Sync Service
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/de_bnetza_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"
Environment="DE_MIRRORS=https://afu.darc.de/bnetza-rufzeichenliste.csv,https://data.hamradiodata.example/de/Rufzeichenliste_AFU.csv"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/de_bnetza_sync de_sync.py

StandardOutput=journal
StandardError=journal
SyslogIdentifier=de.bnetza.sync
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/de.bnetza.sync.timer",
            "content": """\
[Unit]
Description=Run Germany Callbook Sync Hourly

[Timer]
OnCalendar=hourly
Persistent=true
RandomizedDelaySec=5m

[Install]
WantedBy=timers.target
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/dx_firehose

EnvironmentFile=/etc/hams_daemons.env
Environment="WS_PORT=8765"

LimitNOFILE=65535

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/dx_firehose dx_firehose.py

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
            "path": "/opt/hams/systemd/fcc.uls.sync.service",
            "content": """\
[Unit]
Description=Ham Radio FCC ULS Daily Sync Daemon
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
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/fcc_uls_sync

# Odoo JSON2-RPC Credentials
EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"

# Execution via Debian-compliant venv wrapper
ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/fcc_uls_sync fcc_sync.py

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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/ham_dx_daemon

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=dx_daemon_service"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/ham_dx_daemon dx_daemon.py

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
            "path": "/opt/hams/systemd/ised.canada.sync.service",
            "content": """\
[Unit]
Description=Ham Radio ISED Canada Daily Sync Daemon
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
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/ised_canada_sync

# Odoo JSON2-RPC Credentials
EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"

# Execution via Debian-compliant venv wrapper
ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/ised_canada_sync ised_sync.py

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
            "path": "/opt/hams/systemd/ncvec.sync.service",
            "content": """\
[Unit]
Description=NCVEC Question Pool Sync Daemon
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
ReadWritePaths=/opt/hams/spool/ncvec
Type=simple
User=odoo
Group=odoo
WorkingDirectory=/opt/hams/daemons/ncvec_sync

Environment="ODOO_USER=captcha_service_internal"

# Execution via Debian-compliant venv wrapper
ExecStart=/usr/bin/env bash /opt/hams/daemons/run_daemon.sh /opt/hams/daemons/ncvec_sync ncvec_sync.py --url "http://www.ncvec.org/downloads/2022-2026 Tech Pool.txt"

# Resiliency
Restart=always
RestartSec=60
StandardOutput=journal
StandardError=journal
SyslogIdentifier=ncvec.sync

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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/noaa_swpc_sync

# Odoo JSON2-RPC Credentials
EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=space_weather_service"
Environment="POLL_INTERVAL=14400"

# Execution via Debian-compliant venv wrapper
ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/noaa_swpc_sync noaa_swpc_sync.py

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
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/noaa_swpc_sync

# Odoo JSON2-RPC Credentials
EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=space_weather_service"
Environment="POLL_INTERVAL=14400"

# Execution via Debian-compliant venv wrapper
ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/noaa_swpc_sync noaa_swpc_sync.py

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
            "path": "/opt/hams/systemd/nz.rsm.sync.service",
            "content": """\
[Unit]
Description=Ham Radio New Zealand Callbook Sync Service
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/nz_rsm_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"
Environment="NZ_MIRRORS=https://data.hamradiodata.example/nz/callbook.csv,https://mirror2.example.com/nz_callsigns.csv"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/nz_rsm_sync nz_sync.py

StandardOutput=journal
StandardError=journal
SyslogIdentifier=nz.rsm.sync
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/nz.rsm.sync.timer",
            "content": """\
[Unit]
Description=Run New Zealand Callbook Sync Hourly

[Timer]
OnCalendar=hourly
Persistent=true
RandomizedDelaySec=5m

[Install]
WantedBy=timers.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/uk.ofcom.sync.service",
            "content": """\
[Unit]
Description=Ham Radio UK Callbook Sync Service
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/uk_ofcom_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"
Environment="UK_MIRRORS=https://data.hamradiodata.example/uk/callbook.csv,https://mirror2.example.com/uk_callsigns.csv"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/uk_ofcom_sync uk_sync.py

StandardOutput=journal
StandardError=journal
SyslogIdentifier=uk.ofcom.sync
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/uk.ofcom.sync.timer",
            "content": """\
[Unit]
Description=Run UK Callbook Sync Hourly

[Timer]
OnCalendar=hourly
Persistent=true
RandomizedDelaySec=5m

[Install]
WantedBy=timers.target
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/pdns_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=dns_api_service_internal"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/pdns_sync pdns_sync.py

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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/lotw_eqsl_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=logbook_api_service_internal"
Environment="POLL_INTERVAL=86400"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/lotw_eqsl_sync lotw_eqsl_sync.py

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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/amsat_tle_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=satellite_sync_service_internal"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/amsat_tle_sync amsat_sync.py

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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/qrz_scraper

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=onboarding_service_internal"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/qrz_scraper qrz_scraper.py

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
        {
            "path": "/opt/hams/systemd/au.repeater.sync.service",
            "content": """\
[Unit]
Description=Ham Radio Australia Callbook Sync Service
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
User=bruce
Group=bruce
WorkingDirectory=/home/bruce/hams_private_primary/daemons/au_acma_sync

EnvironmentFile=/etc/hams_daemons.env
Environment="ODOO_USER=fcc_sync_service"
Environment="AU_MIRRORS=https://data.hamradiodata.example/au/callbook.csv,https://mirror2.example.com/au_callsigns.csv"

ExecStart=/usr/bin/env bash /home/bruce/hams_private_primary/daemons/run_daemon.sh /home/bruce/hams_private_primary/daemons/au_acma_sync au_sync.py

StandardOutput=journal
StandardError=journal
SyslogIdentifier=au.acma.sync
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
        {
            "path": "/opt/hams/systemd/au.repeater.sync.timer",
            "content": """\
[Unit]
Description=Run Australia Callbook Sync Hourly

[Timer]
OnCalendar=hourly
Persistent=true
RandomizedDelaySec=5m

[Install]
WantedBy=timers.target
""",
            "owner": "root:root",
            "mode": "644",
            "environments": ["prod", "test"],
        },
    ],
    "addon_repos": [
        "hams_community",
        "hams_private_primary",
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
        {"name": "postgresql", "environments": ["early_prod"]},
        {"name": "postgresql-client", "environments": ["early_prod"]},
        {"name": "postgresql-17-pgvector", "environments": ["early_prod"]},
        {"name": "nginx", "environments": ["early_prod"]},
        {"name": "redis-server", "environments": ["early_prod"]},
        {"name": "rabbitmq-server", "environments": ["early_prod"]},
        {"name": "python3-redis", "environments": ["early_prod"]},
        {"name": "python3-pika", "environments": ["early_prod"]},
        {"name": "sqlite3", "environments": ["early_prod"]},
        {"name": "pdns-server", "environments": ["early_prod"]},
        {"name": "pdns-backend-sqlite3", "environments": ["early_prod"]},
        {"name": "kopia", "environments": ["early_prod"]},
        {"name": "pgbackrest", "environments": ["early_prod"]},
        {"name": "certbot", "environments": ["early_prod"]},
        {"name": "python3-certbot-nginx", "environments": ["early_prod"]},
        {"name": "python3-venv", "environments": ["early_prod"]},
        {"name": "python3-passlib", "environments": ["early_prod"]},
        {"name": "python3-cryptography", "environments": ["early_prod"]},
        {"name": "build-essential", "environments": ["early_prod"]},
        {"name": "libpq-dev", "environments": ["early_prod"]},
        {"name": "python3-dev", "environments": ["early_prod"]},
        {"name": "bind9-dnsutils", "environments": ["early_prod"]},
        {"name": "python3-stdeb", "environments": ["early_prod"]},
        {"name": "fakeroot", "environments": ["early_prod"]},
        {"name": "python3-all", "environments": ["early_prod"]},
        {"name": "python3-setuptools", "environments": ["early_prod"]},
        {"name": "dh-python", "environments": ["early_prod"]},
        {"name": "jing", "environments": ["early_prod"]},
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
    os.environ.setdefault(
        "PDNS_API_URL", "http://powerdns:8081/api/v1/servers/localhost/zones"
    )
    os.environ.setdefault("PDNS_API_KEY", "secret")

    if provision_dirs:
        dirs = [
            d["path"] for d in MANIFEST["directories"] if "test" in d["environments"]
        ]
        try:
            for d in dirs:
                os.makedirs(d, exist_ok=True)
                os.chmod(d, 0o777)
                for root, subdirs, files in os.walk(d):
                    for item in subdirs + files:
                        try:
                            os.chmod(os.path.join(root, item), 0o777)
                        except OSError:
                            pass
        except PermissionError:
            print("[*] Elevating briefly to provision required host directories...")
            subprocess.run(["sudo", "mkdir", "-p"] + dirs, check=True)
            subprocess.run(["sudo", "chmod", "-R", "777"] + dirs, check=True)


def get_mount_paths(environment, mount_type):
    return [
        d["path"]
        for d in MANIFEST["directories"]
        if environment in d["environments"] and d.get("runtime_mount") == mount_type
    ]


def execute_hooks(environment, run_cmd_func, env_vars=None, dest_dir=""):
    if dest_dir and dest_dir.endswith("/"):
        dest_dir = dest_dir[:-1]

    fmt_vars = {}
    if env_vars:
        fmt_vars.update(env_vars)
    fmt_vars["DEST_DIR"] = dest_dir

    for d in MANIFEST["directories"]:
        if environment in d["environments"] and "post_provision_hooks" in d:
            for hook in d["post_provision_hooks"]:
                cmd = hook
                try:
                    cmd = cmd.format(**fmt_vars)
                except KeyError:
                    pass
                run_cmd_func(cmd, env=env_vars)


def apply_production_directories(run_cmd_func, environment="prod", dest_dir=""):
    for d in MANIFEST["directories"]:
        if environment in d["environments"]:
            path = d["path"]
            if dest_dir:
                path = os.path.join(dest_dir, path.lstrip("/"))
            owner = d["owner"]
            mode = d["provision_mode"]

            run_cmd_func(["mkdir", "-p", path])
            run_cmd_func(["chown", "-R", owner, path])
            run_cmd_func(["chmod", "-R", mode, path])


def write_env_files(base_etc_dir, env_vars, run_cmd_func, dest_dir=""):
    if dest_dir:
        base_etc_dir = os.path.join(dest_dir, base_etc_dir.lstrip("/"))
    run_cmd_func(["mkdir", "-p", base_etc_dir])
    for filename, keys in MANIFEST["env_groups"].items():
        filepath = os.path.join(base_etc_dir, filename)
        content = ""
        for k in keys:
            if k in env_vars:
                content += f"{k}={env_vars[k]}\n"

        flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
        fd = os.open(filepath, flags, 0o400)
        with open(fd, "w", encoding="utf-8") as f:
            f.write(content)

        run_cmd_func(["chmod", "400", filepath])
        run_cmd_func(["chown", "root:root", filepath])


def provision_apt_packages(run_cmd_func, environment="prod"):
    packages = []
    for pkg_spec in MANIFEST.get("apt_packages", []):
        if environment in pkg_spec["environments"]:
            packages.append(pkg_spec["name"])

    if packages:
        run_cmd_func(["apt-get", "update"])
        run_cmd_func(["apt-get", "install", "-y"] + packages)


def provision_custom_addons(run_cmd_func, env_vars, environment="prod", dest_dir=""):
    if environment not in ["prod", "test"]:
        return

    repos = MANIFEST.get("addon_repos", [])
    if not repos:
        return

    repo_root = env_vars.get("REPO_ROOT")
    if not repo_root:
        return

    base_dir = os.path.abspath(os.path.join(repo_root, ".."))
    custom_addons_dir = "/opt/hams/odoo"
    if dest_dir:
        custom_addons_dir = os.path.join(dest_dir, custom_addons_dir.lstrip("/"))

    for repo_name in repos:
        repo_path = os.path.join(base_dir, repo_name)
        if repo_name == os.path.basename(repo_root):
            repo_path = repo_root

        if os.path.isdir(repo_path):
            for item in os.listdir(repo_path):
                item_path = os.path.join(repo_path, item)
                if os.path.isdir(item_path) and os.path.exists(
                    os.path.join(item_path, "__manifest__.py")
                ):
                    target = os.path.join(custom_addons_dir, item)
                    run_cmd_func(["/bin/bash", "-c", f"rm -rf {target}"])
                    run_cmd_func(["mkdir", "-p", target])
                    run_cmd_func(["cp", "-a", f"{item_path}/.", target])

    run_cmd_func(["chown", "-R", "odoo:odoo", custom_addons_dir])


def provision_python_venvs(run_cmd_func, environment="prod", dest_dir=""):
    for venv_spec in MANIFEST.get("python_venvs", []):
        if environment not in venv_spec["environments"]:
            continue

        venv_path = venv_spec["path"]
        requirements_file = venv_spec.get("requirements_file")

        if dest_dir:
            if not venv_path.startswith("/"):
                venv_path = os.path.join(dest_dir, venv_path)
            if requirements_file and not requirements_file.startswith("/"):
                requirements_file = os.path.join(dest_dir, requirements_file)

        if not os.path.exists(venv_path):
            cmd = ["/usr/bin/python3", "-m", "venv"]
            if venv_spec.get("system_site_packages"):
                cmd.append("--system-site-packages")
            cmd.append(venv_path)
            run_cmd_func(cmd)

        pip_exe = os.path.join(venv_path, "bin", "pip")
        if requirements_file:
            if not os.path.exists(requirements_file):
                raise FileNotFoundError(
                    f"Required requirements file not found: {requirements_file}"
                )
            run_cmd_func([pip_exe, "install", "-r", requirements_file])

            # Ensure Playwright browser binaries are installed for SPA scraping
            playwright_exe = os.path.join(venv_path, "bin", "playwright")
            if os.path.exists(playwright_exe):
                run_cmd_func(
                    [
                        "env",
                        "PLAYWRIGHT_BROWSERS_PATH=/opt/hams/cache/ms-playwright",
                        playwright_exe,
                        "install",
                        "chromium",
                    ]
                )


def provision_static_files(run_cmd_func, env_vars, environment="prod", dest_dir=""):
    for file_spec in MANIFEST.get("static_files", []):
        if environment not in file_spec["environments"]:
            continue

        condition_env = file_spec.get("condition_env")
        if condition_env and not env_vars.get(condition_env):
            continue

        path = file_spec["path"]
        try:
            path = path.format(**env_vars)
        except KeyError:
            pass

        if dest_dir:
            path = os.path.join(dest_dir, path.lstrip("/"))

        # Automatically create base directories to support dynamic structural generation
        os.makedirs(os.path.dirname(path), exist_ok=True)

        src = file_spec.get("src")
        url = file_spec.get("url")

        if src:
            try:
                src = src.format(**env_vars)
            except KeyError:
                pass
            if os.path.exists(src):
                if not os.path.exists(path):
                    os.makedirs(path, exist_ok=True)
                run_cmd_func(["cp", "-a", src, path])
        elif url:
            try:
                if (
                    "{DEB_TARGET_ARCH_CPU}" in url
                    and "DEB_TARGET_ARCH_CPU" not in env_vars
                ):
                    res = subprocess.run(
                        ["dpkg-architecture", "-q", "DEB_TARGET_ARCH_CPU"],
                        capture_output=True,
                        text=True,
                    )
                    if res.returncode == 0:
                        env_vars["DEB_TARGET_ARCH_CPU"] = res.stdout.strip()
                url = url.format(**env_vars)
            except KeyError:
                pass
            ua = env_vars.get(
                "SYSTEM_USER_AGENT",
                "Hams.com Bruce Perens K6BP <bruce@perens.com> +1 510-394-5627",
            )
            req = urllib.request.Request(url, headers={"User-Agent": ua})
            try:
                with urllib.request.urlopen(req) as response:
                    data = response.read()
            except Exception: # Network partition fallback safety
                data = b""

            flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
            fd = os.open(path, flags, int(file_spec.get("mode", "644"), 8))
            with open(fd, "wb") as f:
                f.write(data)
        else:
            content = file_spec.get("content", "")
            try:
                content = content.format(**env_vars)
            except KeyError:
                pass

            flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
            fd = os.open(path, flags, int(file_spec.get("mode", "644"), 8))
            with open(fd, "w", encoding="utf-8") as f:
                f.write(content)

        if "mode" in file_spec:
            if os.path.isdir(path):
                run_cmd_func(["chmod", "-R", file_spec["mode"], path])
            else:
                run_cmd_func(["chmod", file_spec["mode"], path])
        if "owner" in file_spec:
            if os.path.isdir(path):
                run_cmd_func(["chown", "-R", file_spec["owner"], path])
            else:
                run_cmd_func(["chown", file_spec["owner"], path])

        if "post_provision_hooks" in file_spec:
            fmt_vars = {}
            if env_vars:
                fmt_vars.update(env_vars)
            fmt_vars["DEST_DIR"] = dest_dir
            fmt_vars["PATH"] = path
            for hook in file_spec["post_provision_hooks"]:
                try:
                    cmd_str = hook.format(**fmt_vars)
                except KeyError:
                    cmd_str = hook
                run_cmd_func(["/bin/bash", "-c", cmd_str])


def generate_odoo_override_conf(odoo_conf_path):
    spec = MANIFEST["systemd_odoo_override"]

    lines = ["[Unit]"]
    for k, v in spec["Unit"].items():
        lines.append(f"{k}={v}")

    lines.append("")
    lines.append("[Service]")
    lines.append(f'Environment="ODOO_RC={odoo_conf_path}"')

    for env_file in spec["Service"]["EnvironmentFiles"]:
        lines.append(f"EnvironmentFile=-/opt/hams/etc/{env_file}")

    for env in spec["Service"]["Environment"]:
        lines.append(f'Environment="{env}"')

    lines.append(f"ProtectSystem={spec['Service']['ProtectSystem']}")

    rw_paths = get_mount_paths("prod", "rw")
    lines.append(f"ReadWritePaths=/var/lib/odoo /var/log/odoo {' '.join(rw_paths)}")

    ro_paths = get_mount_paths("prod", "ro")
    lines.append(f"ReadOnlyPaths={' '.join(ro_paths)}")

    lines.append(f"BindPaths={spec['Service']['BindPaths']}")
    lines.append(f"PrivateTmp={spec['Service']['PrivateTmp']}")
    lines.append(f"PrivateDevices={spec['Service']['PrivateDevices']}")
    lines.append(f"NoNewPrivileges={spec['Service']['NoNewPrivileges']}")
    lines.append(f"KillSignal={spec['Service']['KillSignal']}")
    lines.append(f"TimeoutStopSec={spec['Service']['TimeoutStopSec']}")
    lines.append("ExecStart=")
    lines.append(
        f"ExecStart=/opt/hams/.venv/bin/python /usr/bin/odoo -c {odoo_conf_path}"
    )

    return "\n".join(lines) + "\n"
