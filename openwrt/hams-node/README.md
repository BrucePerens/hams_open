# hams-node (AREDN gateway package)

OpenWrt package (GPL-2.0-or-later) for AREDN 4.26.x nodes (OpenWrt 25.12). AREDN installs packages from the
node's admin package store (Package settings, Upload Package), not with opkg, so the build output is an `.apk`
to upload. Mesh-to-WAN is off by default on AREDN; this package does not change it.

Build (verified with the x86-64 SDK, `hams-node-0.1.0-r1.apk`; unpack the SDK in an empty directory):

    echo "src-link hams <repo>/openwrt" > feeds.conf
    ./scripts/feeds update hams && ./scripts/feeds install hams-node
    make defconfig && echo CONFIG_PACKAGE_hams-node=m >> .config && make defconfig
    make package/hams-node/compile        # bin/packages/<arch>/hams/hams-node-*.apk

The package is architecture independent (shell scripts); use the SDK of the node's target for the dependency
metadata.

Contents: `hams-node-siit` renders one Jool SIIT instance file per mesh domain from the registry export (the same
files as `ipv6_siit/siit_map.py gateway_configs`) and loads them; `hams-node-wg` adds WireGuard
peers on Internet links only.

`hams-node-ondemand` (off by default, `/etc/config/hams_node_ondemand`): an nft set `inet hams_ondemand seen` (keyed on the /60,
timeout = idle minutes, refreshed by every forwarded packet to `ham_prefix`) is the traffic detector. A new element is the first packet:
the daemon asks the members-only directory lookup (member certificate), checks the record is for that /60, and adds a WireGuard peer
(`AllowedIPs` = the /60). A peer whose /60 has left the set (10 idle minutes) is removed. Unknown, suspended or stale answers create
nothing and are not asked again for 60 s. At most `max_peers` (256) peers; the oldest idle one goes first. An `rf` link is refused.
Not exercised on a real device or in the AREDN VM; the nft ruleset and `nft monitor` parsing are untested here.

Tests: `python3 -m unittest discover -s openwrt/hams-node/tests` (a stand-in `jsonfilter` is used on the dev box).

## Tested on a virtual AREDN node (2026-11-23)

Image: `aredn-4.26.10.0-x86-64-generic-ext4-combined.img.gz` from `downloads.arednmesh.org/releases/4/26/4.26.10.0/targets/x86/64/`
(sha256 in `sha256sums`; kernel 6.12.94, apk-tools 3.0.5, busybox 1.37, real `jsonfilter`). QEMU under the build user, no root:

    gunzip -k aredn-*.img.gz; cp aredn-*.img disk.img; qemu-img resize -f raw disk.img 256M
    qemu-system-x86_64 -enable-kvm -cpu host -m 512 -smp 2 -display none -monitor none \
      -serial unix:ser.sock,server,nowait -drive file=disk.img,format=raw,if=virtio \
      -netdev user,id=n0,net=192.168.1.0/24,host=192.168.1.2 -device virtio-net-pci,netdev=n0 \
      -netdev user,id=n1 -device virtio-net-pci,netdev=n1

First boot: the node answers on the LAN only at its 10.x/29 address, so use the serial console (a `socket` client on
`ser.sock`). Set name and password with `curl -X PUT -d 'name=N0CALL-vm&passwd=...' http://<lan>/a/status` (done through the forwarded
LAN port before the node reconfigured itself), then it reboots. The WAN is VLAN 1 tagged on eth0 only; to give it
internet put the second NIC in: add `list ports 'eth1:u'` under the `option vlan '1'` bridge-vlan in `/etc/config/network` and
`/etc/init.d/network restart` (br-wan then gets 10.0.2.15 by DHCP and 10.0.2.2 is the host). Serve files from the host with
`python3 -I -m http.server --bind 127.0.0.1` and fetch them with `wget http://10.0.2.2:PORT/...`.

Results:

- Install: `apk add --allow-untrusted ./hams-node-0.1.0-r1.apk` (what Package settings does) is refused: **the image has no
  `jool-tools-siit` or `kmod-jool-siit`** (not in the AREDN feeds; no `jool` module in `/lib/modules/6.12.94`), so Jool SIIT cannot run on this
  image. With stub packages for those two names the apk installs, `/etc/init.d/hams-node start` runs, and the files land.
- `hams-node-siit render` with the real `jsonfilter`: the Jool instance file is correct; a null `prefix96` row is skipped, a mesh name
  with `;` is skipped with a message. `jool_siit file handle` could not be run (no Jool). `apply` exits 0 even when `jool_siit`
  fails (it only prints), which hides the failure from procd.
- WireGuard: kmod-wireguard and wireguard-tools are in the image. `hams-node-wg apply wgt0 internet peers` added the peer and a
  handshake completed against a second local interface (loopback and 10.0.2.15 endpoints); `rf` is refused. Bug found and fixed: the
  bad-character check never matched under busybox ash (hams_open #3227). AREDN policy routing (`ip rule` lookups of tables 20 to 28,
  table 28 holds 0.0.0.0/1 and 128.0.0.0/1 via the WAN) beats the main table: a route for a WG-reachable prefix must be given a
  rule of priority below 100 (`ip rule add pref 100 to PREFIX lookup main`) or it is never used.
- AREDN services: babeld, dnsmasq, arednlink, uhttpd and dropbear stayed up before and after install and start.
- Part 97 boundary (`ham_tls/ham_boundary.py`): `nft -f` **fails on this image** because it has no `nfnetlink_log` (`log group`;
  hams_open #3229 adds `log_backend: syslog`). With that, in the `output` hook toward br-wan (a VM without veth or dummy cannot
  forward between two interfaces): plain tcp 28999 passed and was logged, tcp 443 and 22 were dropped and counted in
  `refused_encrypted`, everything else counted in `refused_other`, AREDN services unaffected.
- `ham_proxy` (musl, `--features vendored`, SDK cross gcc): 6,090,240 byte static binary; RSS 4.5 MB idle, 6.5 MB peak after
  8 parallel clients (400 requests, about 290 requests/s in the 2-vCPU guest); see hams_com `docs/runbooks/HAM_PROXY_IMPLEMENTATION_BASE.md`.

## Enrolment (`hams-node-enroll`, separate package)

`hams-node-enroll enroll` asks hams.com for a code, prints it, and waits while the member approves it at `/my/ipv6` (or
`hams-node-enroll enroll TOKEN` with a one-time token made there). The router then keeps only a revocable device token
(`/etc/hams-node/device_token`, mode 0600) and its own WireGuard key (`wg.key`, 0600, made on the device); the member's
login secret is never typed into or stored on the router. `hams-node-enroll fetch` (a cron line, every 6 hours, added by
enrol) renews the configuration into `/etc/config/hams_node` and `hams_node_ondemand`. A revoked token stops the node:
exit 3, the cron line and token are removed, and no request is made again until the member enrols anew.

## Reporting hosts and services (`hams-node-report`, separate package)

`hams-node-report start` adds a cron line (every 30 minutes) that runs `hams-node-report run`: it reads the AREDN host list
(`/var/run/hosts_olsr`) and service ads (`/var/run/services_olsr`) and POSTs them to `/api/ipv6/gateway/report` with the
enrolled device token (`/etc/hams-node/device_token`, mode 0600) over HTTPS. hams.com publishes them as
`<host>.<mesh>.<callsign>.ham` and drops them 24 hours after the reports stop. Exit codes: 0 reported, 1 refused or not
enrolled, 3 token revoked, 4 hams.com unreachable (cron retries).
