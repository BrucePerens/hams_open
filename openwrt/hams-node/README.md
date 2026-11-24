# hams-node (AREDN gateway package)

OpenWrt package (GPL-2.0-or-later) for AREDN 4.26.x nodes (OpenWrt 25.12). AREDN installs packages from the
node's admin package store (Package settings, Upload Package), not with opkg, so the build output is an `.apk`
to upload. Mesh-to-WAN is off by default on AREDN; this package does not change it.

Build (verified with the x86-64 SDK, `hams-node-0.1.0-r1.apk`; unpack the SDK in an empty directory):

    printf 'src-git packages https://git.openwrt.org/feed/packages.git^<commit in the SDK feeds.conf.default>\nsrc-link hams <repo>/openwrt\n' > feeds.conf
    ./scripts/feeds update packages hams && ./scripts/feeds install hams-node
    make defconfig && echo CONFIG_PACKAGE_hams-node=m >> .config && make defconfig
    make package/hams-node/compile        # bin/packages/<arch>/hams/hams-node-*.apk

The `packages` feed line is needed by `hams-node-mtls`: nginx with TLS is `nginx-ssl` there (`nginx-full` also has SSL but is larger; the
old one-line feeds.conf replaced the SDK's default feeds, which is why `nginx-ssl` did not resolve before). To build only `hams-node-mtls` and
its siblings without compiling Jool, set `CONFIG_PACKAGE_hams-node-mtls=m` in `.config` and run
`make -C package/feeds/hams/hams-node compile TOPDIR=$PWD` (bin/packages/<arch>/hams/hams-node-mtls-*.apk; its depends include `nginx-ssl`,
checked with `apk adbdump`).

The package is architecture independent (shell scripts); use the SDK of the node's target for the dependency
metadata.

Contents: `hams-node-siit` renders one Jool SIIT instance file per mesh domain from the registry export (the same
files as `ipv6_siit/siit_map.py gateway_configs`) and loads them; `hams-node-wg` adds WireGuard
peers on Internet links only; `hams-node-rf-route apply|down IFACE rf|internet PREFIXES` gives RF routes metric 100 and tunnel routes metric 200 (RF first, tunnel as backup) and keeps end-to-end encrypted traffic off RF: `ham_boundary` marks it (fwmark 151) and a rule sends it to table 197, which holds tunnel routes only. It does nothing for an empty prefix list.

`hams-node-ondemand` (off by default, `/etc/config/hams_node_ondemand`): an nft set `inet hams_ondemand seen` (keyed on the /60,
A successful lookup is cached in the state directory (`directory.cache`); with hams.com unreachable the cached entry is used, so a tunnel re-connects without the internet. An explicit not-found/suspended answer drops the entry.
timeout = idle minutes, refreshed by every forwarded packet to `ham_prefix`) is the traffic detector. A new element is the first packet:
the daemon asks the members-only directory lookup (member certificate), checks the record is for that /60, and adds a WireGuard peer
(`AllowedIPs` = the /60). A peer whose /60 has left the set (10 idle minutes) is removed. Unknown, suspended or stale answers create
nothing and are not asked again for 60 s. At most `max_peers` (256) peers; the oldest idle one goes first. An `rf` link is refused.
Run in the QEMU test image with real peers (`image/tools/ondemand_test.py`, see `image/README.md`). The set is polled once a second (`nft monitor` sends no event for an element a packet adds to a dynamic set), so a first packet waits at most a second for its peer. Memory limit: all `hams-node-ondemand` processes together stay under 8 MB RSS with 256 peers (measured about 2 MB).

Adding a subpackage: create `pkg/<name>.mk` with its `define Package/<name>`, `/description`, `/conffiles` and `/install` blocks
(copy `pkg/hams-node-report.mk`; the file name is the package name). Do not edit `Makefile`: it includes `pkg/*.mk` in sorted order
and calls `BuildPackage` for each, so parallel subpackage changes never touch the same file.

## RF detection (hams-node-rf)

`hams-node-rf scan` runs at boot and on every interface up/down. An interface is RF when listed in `hams_node_fw.main.rf_iface` (the one
setting) or when `iw dev` reports it as a mesh point or IBSS. It goes into zone `hamrf` (WireGuard and encrypted-service ports refused,
Part 97), `hams_node_bird.main.rf_live` becomes 1 while one is up (routes carry the RF community), a station ID reminder is written to
`/var/run/hams-node/station_id_reminder` and the system log, and the link-local neighbours seen on RF interfaces are listed in
`/var/run/hams-node/rf_neighbours` (`hams-node-rf list`). `/etc/hams-node/gossip_neighbours` is left alone.

## Firewall: the home LAN is outbound-only (hams-node-fw)

Safe by default: once the node is enrolled the zones are on (`option enabled '0'` in `/etc/config/hams_node_fw` switches them off). `hams-node-wg`
and `hams-node-ondemand` put their WireGuard interface into zone `ham`, `hams-node-rf-route` puts an rf interface into zone `hamrf` (both through
`hams-node-fw ensure`); both zones have input and forward DROP. `hams-node-fw check` fails for any WireGuard interface that is up but in no zone.

- `lan` -> `ham`: forwarded (replies come back through connection tracking). No forwarding `ham` -> `lan` or `hamrf` -> `lan`, plus an explicit
  DROP rule for new connections from both zones to `lan`. LAN hosts are outbound-only; nothing on the LAN is reachable from the ham network.
- `list open 'tcp:PORT'` opens a port on the router itself only (zone input). There is no default way to expose a LAN host.
- A member who wants to expose one host adds an explicit rule by hand (this is not offered in the UI):

      uci add firewall rule
      uci set firewall.@rule[-1].name='expose-nas-https'
      uci set firewall.@rule[-1].src='ham'
      uci set firewall.@rule[-1].dest='lan'
      uci set firewall.@rule[-1].dest_ip='192.168.1.50'
      uci set firewall.@rule[-1].proto='tcp'
      uci set firewall.@rule[-1].dest_port='8443'
      uci set firewall.@rule[-1].target='ACCEPT'
      uci commit firewall; /etc/init.d/firewall reload

  (a rule placed after the package's DROP rule needs `uci reorder`; never toward `hamrf` for an encrypted service, Part 97.)
- Test: `openwrt/image/tools/fw_isolation_test.py IMAGE` boots four OpenWrt VMs (router, outsider on the ham side, LAN host, hamlan host).

### Optional third zone: hamlan (a segment reachable from the ham network)

Nothing exists unless the member designates it in `/etc/config/hams_node_fw`: `option hamlan_vlan '42'` (a VLAN on `br-lan`, or on
`hamlan_parent`) or `list hamlan_port 'lan4'` (those ports leave `br-lan` and form `br-hamlan`); not both. `hams-node-fw apply` creates
interface `hamlan`, adds it to `hams_node_lan` so it gets its own /64 of the /60, and zone `hamlan` (input and forward DROP; router advertisements
and DHCPv6 are allowed in). Forwardings: `lan` -> `hamlan`, `hamlan` -> `ham`, `ham` -> `hamlan`, `hamrf` -> `hamlan` (encrypted-service ports are
dropped toward RF, Part 97). `hamlan` -> `lan` is dropped: strict isolation from the home LAN, which stays outbound-only. To restrict inbound to
listed hosts, `list hamlan_open '2001:db8:ab40:11::10/443'` (tcp) or `'ADDR/udp:5000'`; with any entry set, only those host/port pairs are reachable.

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

- Install: `apk add --allow-untrusted ./hams-node-0.1.0-r1.apk` (what Package settings does) is refused: **the image has no Jool**
  (`jool-tools-netfilter` and `kmod-jool-netfilter`, the 25.12 feed names; not in the AREDN feeds; no `jool` module in `/lib/modules/6.12.94`).
  See "Jool on AREDN" below. With stub packages for those two names the apk installs, `/etc/init.d/hams-node start` runs, and the files land.
- `hams-node-siit render` with the real `jsonfilter`: the Jool instance file is correct; a null `prefix96` row is skipped, a mesh name
  with `;` is skipped with a message. `apply` used to exit 0 when `jool_siit` failed; fixed (#3235).
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

`hams-node-enroll unenroll` revokes the device token on hams.com (`/api/ipv6/enroll/unenroll`, own device only), then removes the
WireGuard peers and interface, the `hams_node` and `hams_node_ondemand` settings, the key, the token and the cron line. If hams.com
cannot be reached it exits 4 and removes nothing; run it again. A token hams.com already refuses is dropped and the rest removed.

## Automatic update (`hams-node-update`, separate package)

Off until `/etc/config/hams_node_update` has a `feed_url` (https; the directory with `packages.adb`) and a `public_key` (path of the
feed's public key PEM, from `openwrt/feed`). `hams-node-update schedule` adds a daily cron line; `hams-node-update run` is one pass:
it installs the key in `/etc/apk/keys` and the feed in `/etc/apk/repositories.d/hams.list` (apk refuses an index not signed by that
key), downloads newer versions of the listed packages into a staging directory (a failed download changes nothing), backs up
`/etc/config/hams_node*`, installs the staged files, then checks each package is at the new version, the settings parse and the
optional `health_cmd` succeeds. Any failure reinstalls the previous versions from the feed and restores the settings (exit 1; the
state is in `/var/lib/hams-node-update/status`). The feed must therefore keep the previous version of each package: with no way back
the update is not started. Tested with the real apk against a signed feed over loopback HTTP; not yet on the QEMU image.

## Reporting hosts and services (`hams-node-report`, separate package)

`hams-node-report start` adds a cron line (every 30 minutes) that runs `hams-node-report run`: it reads the AREDN host list
(AREDN 4.x has no OLSR: `/etc/arednlink/hosts` and `/var/run/arednlink/hosts/*`, one file per node) and service ads (`/etc/arednlink/services`, `/var/run/arednlink/services/*`; AREDN's `pseudo` reachability entries are skipped) and POSTs them to `/api/ipv6/gateway/report` with the
enrolled device token (`/etc/hams-node/device_token`, mode 0600) over HTTPS. hams.com publishes them as
`<host>.<mesh>.<callsign>.ham` and drops them 24 hours after the reports stop. Exit codes: 0 reported, 1 refused or not
enrolled, 3 token revoked, 4 hams.com unreachable (cron retries).

## Per-peer traffic totals (`hams-node-traffic`, separate package)

`hams-node-traffic start` adds an hourly cron line that runs `hams-node-traffic run`: it reads `wg show IFACE transfer` (default
interface `hams0`, `-i` changes it) and POSTs to `/api/ipv6/traffic/report`, with the device token, only each peer's WireGuard
public key, bytes received, bytes sent since the last report and the interval in seconds. The first run only records a baseline. No
content, flows, endpoints or addresses are read or sent; hams.com keeps daily totals per member pair, to see which pairs are busy
enough to move to a radio link. `hams-node-traffic optout` (creates `/etc/hams-node/no_traffic`) stops reporting; `optin` resumes;
members can also opt out on `/my/ipv6`, after which hams.com drops their rows. Exit codes as `hams-node-report`.

## Second round on the virtual AREDN node, and stock OpenWrt (2026-11-23)

### Jool on AREDN
- OpenWrt 25.12 packages Jool as `kmod-jool-netfilter` and `jool-tools-netfilter` (one kernel module set and one tools package that holds
  `jool_siit`); `hams-node` now depends on those (#3248). Build with the 25.12.5 SDK: add the `packages` feed to `feeds.conf`, install both,
  `make package/jool/compile`; the SDK feeds lack `libnl` and `argp-standalone` (copy `package/libs/libnl` and `package/libs/argp-standalone`
  from the openwrt tree at the SDK's commit).
- Kernel match (superseded 2026-11-23, see `openwrt/aredn-jool/README.md`): a kmod from the stock SDK fails every Jool netlink reply
  (`genlmsg_reply() failed (errcode -111)`) because the SDK kernel has `CONFIG_NET_NS=y` and AREDN's has not, which shifts `struct genl_info`
  under the inline `genlmsg_reply()`. Jool built from AREDN's own tree (`openwrt/aredn-jool/build_aredn_jool.sh`, per target) loads and works on the stock image.
- Stock OpenWrt 25.12.5 x86-64 in QEMU: `apk add` pulls Jool from the feed, the real `jool_siit` accepts the files rendered by
  `hams-node-siit render` (`jool_siit -i mesh7 global display` shows `pool6: 2001:db8:ab01:7::/96`), `apply` replaces the instance and exits 1
  when an instance file is invalid.

### hams-node-wg, hams-node-ondemand on AREDN
- `ip rule add to PREFIX lookup main priority 90` works on AREDN's `ip` and sits below its priority-100 rules; `hams-node-wg down` removes it (#3234).
- `hams-node-ondemand`: the nft table did not load (`chain fwd` is an nft keyword; statements need a newline before the closing brace; fixed in
  #3243). After that: the ruleset loads on AREDN's nft; a `nft monitor new elements` event for a member /60 (set element added by hand, since a
  one-host VM has no forward path) makes the daemon look the /60 up (stub lookup API), add the WireGuard peer with allowed-ips the /60, and
  write `peers`; with `idle_minutes 1` the peer and its state line were removed within about two minutes (`expire` loop every 60 s). RSS:
  daemon shell 0.8 MB, its pipeline subshell 1.1 MB, `nft monitor` 3.1 MB, the expire loop shell about 1 MB: about 6 MB in total.
  uclient-fetch sends the POST body chunked (no Content-Length): the lookup server must accept that.
- `/usr/libexec/rpcd` does not exist on AREDN: create it before installing the rpcd plugin of luci-app-hams-node.
- `hams-node-report` read OLSR files that AREDN 4 does not have; it now reads `/etc/arednlink/{hosts,services}` and
  `/var/run/arednlink/{hosts,services}/*` (#3249).
- LuCI is not in the AREDN image, so luci-app-hams-node was checked on stock OpenWrt 25.12.5 (see the luci-app to-do).

### Jool on stock AREDN 4.26.10.0 (solved 2026-11-23)
Built from AREDN's own tree (github.com/aredn/aredn tag 4.26.10.0, OpenWrt f5dae5ece4, release `config.buildinfo`; `./scripts/feeds install jool libnl`;
`CONFIG_PACKAGE_kmod-jool-netfilter=m`, `CONFIG_PACKAGE_jool-tools-netfilter=m`; `make package/kernel/linux/compile package/jool/compile package/libnl/compile`),
the modules load and work on the UNMODIFIED stock image (no custom firmware). Needed apks: kmod-jool-netfilter, jool-tools-netfilter, kmod-crypto-md5
and the six libnl packages (libnl200, -core200, -genl200, -cli200, -nf200, -route200; the AREDN store has only libnl-tiny). Install:
`apk add --allow-untrusted ./*.apk` resolves all dependencies. The kmod depends on `kernel=6.12.94~5a659643ce3911a4604e3e1c36ff86d9-r1` (the node's
`apk list -I kernel`); a rebuild gets a different hash (0fa03e...), so write the node's hash into `build_dir/.../linux-6.12.94/.vermagic` before building
the kmods. Test: three QEMU VMs (v6 host, stock AREDN with Jool, v4 host; `-netdev socket`): `ping -6` and `wget` through pool6 `2001:db8:c::/96` worked, and
`hams-node-siit render` + `apply` created the instance. The earlier SDK-built module (`genlmsg_reply -111`) is explained by the NET_NS mismatch (`openwrt/aredn-jool/README.md`); the tree-built one works.
Each other target/release needs its own kmod build: `openwrt/aredn-jool/build_aredn_jool.sh -t TARGET` (it verifies kernel hash, config and module ABI against the release and refuses otherwise).

## /60 to the LANs (`hams-node-lan`, separate package)

`hams-node-lan apply` (run by `hams-node-enroll` after every enrol and renewal when installed) declares the member's /60
(`hams_node.main.prefix60`) as the `ip6prefix` of a static interface `network.hamnet`, gives every interface listed in
`hams_node_lan.main.interface` (default `lan`, at most 16) `ip6assign 64` and its own `ip6hint` (0, 1, 2 ...), and sets
`dhcp.<if>.ra` and `dhcpv6` to `server`, so netifd cuts one /64 per LAN out of the /60 and odhcpd sends router advertisements
and answers DHCPv6. `hams-node-lan remove` undoes it. Anything that is not a /60 changes nothing. Not exercised on a real router.

## Management only with a member client certificate (`hams-node-mtls`, separate package)

Bruce 2026-11-23: once a node is enrolled, its web management (LuCI, ubus over HTTP so rpcd and the hams-node APIs) is reachable only
with a hams.com member (client) certificate. `hams-node-mtls apply` (run by `hams-node-enroll` after enrol and every renewal) moves
uhttpd to `127.0.0.1:8080` and starts nginx on port 443 with `ssl_verify_client on`: the certificate must chain to the pinned member
CA (`/etc/hams-node/member-ca.pem`: Root A, Root B, Identity Root, Identity Issuing CA, copied from hams_com `daemons/relay_ca/public_certs`)
and its Subject callsign (`1.3.6.1.4.1.66926.1.1`) must equal the enrolled owner's callsign (`/etc/hams-node/owner`, from the enrolment
reply). No certificate, another member's, another CA's or a revoked one is refused. The router's TLS certificate is its own
self-signed one (pin it or use `curl -k`); use the member certificate with `curl --cert member.pem --key member.key https://ROUTER/`
(`docs/runbooks/MEMBER_CLIENT_CERTIFICATE.md` in hams_com).

- Revocation: the shipped CA-level CRLs plus the identity CRL, fetched from `/identity-ca/crl` every 6 hours by cron
  (`hams-node-mtls crl`, signature checked against the pinned CAs, last good copy kept). While a CRL is past its `nextUpdate` (the node
  was offline from hams.com) it is not applied, so an offline owner is not locked out; revocation is then enforced again on the next fetch.
- Not enrolled, or after `unenroll`/`hams-node-mtls off`: uhttpd is put back as it was and the stock login is unchanged (first-boot setup).
- Single sign-on, no router password: once nginx has verified the owner certificate, LuCI's login page (403) is redirected to `/hams-sso`, a
  loopback-only CGI (uhttpd `hamssso` on 127.0.0.1:8081, reachable only through nginx) that opens an rpcd session for the internal login
  `hams-owner` (random secret in `/etc/hams-node/mtls/sso.secret`) and sets the LuCI cookie. After the first owner sign-in the stock rpcd
  logins (root) are removed (`hams-node-mtls lock`), so no password works for LuCI or ubus; `unenroll`/`off` restores them. SSH is a separate
  to-do (key-only).
- Recovery if the owner certificate is lost or the member's callsign changes hands: a physical reset (`firstboot`, or the reset button) restores
  the stock login, then enrol again. Nothing remote can remove the requirement.
- Tested on the OpenWrt 25.12.5 x86-64 image in QEMU with a test CA (`image/tools/mtls_test.py`): owner 200, another member 403, none, revoked and
  other-CA refused, uhttpd only on loopback, owner opens LuCI without a password, other/no certificate refused at `/hams-sso`, root password login refused after the first sign-in and back after `off`. The `.apk` is built by the SDK from `pkg/hams-node-mtls.mk` (see Build).

## Endpoint updates (`hams-node-endpoint`, separate package)

`hams-node-endpoint update` publishes `<WAN address>:51820` (uci `hams_node_endpoint.main`: `interface`, `port`, `host`) to
`/api/ipv6/directory/endpoint` with the portal's signed-update format: HMAC-SHA256 over `public_key\nendpoint\ntimestamp` under the
X25519 shared secret of the node's key (`/etc/hams-node/wg.key`, made on the device) and the portal's key (`hams_node.main.server_key`).
openssl reads the private key from its root-only file; it is in no request, log or argument. It sends when the address changed
or the last update is over 6 hours old (`/etc/hams-node/endpoint_sent`); a hotplug script (`ifup`/`ifupdate` of the WAN) runs it at once
on a WAN address change, and a cron line (`start`, every 30 minutes, added when the package is installed) refreshes it. Exit 0 sent or
nothing to do, 1 refused or not enrolled, 4 hams.com unreachable. Behind CGNAT the WAN address is not reachable; set `host`.

## Key-only SSH (`hams-node-ssh`, separate package)

Once the node is enrolled (`hams-node-enroll` runs `hams-node-ssh apply` after enrol and every renewal) and `/etc/dropbear/authorized_keys`
holds at least one key, dropbear's `PasswordAuth` and `RootPasswordAuth` are set to off on every instance. With no key installed nothing
changes, so the owner cannot lock themselves out; add the key in LuCI (System > Administration > SSH-Keys), which `hams-node-mtls` limits
to the owner's member certificate. `unenroll` (or `hams-node-ssh off`) restores the saved settings. Recovery after losing the key: physical
reset, then enrol again. The owner's exported member certificates also sign in over SSH (Bruce, 2026-11-23: no separate SSH CA; design
`docs/proposals/SSH_CERTIFICATES.md`, option A): in My certificates the member exports a certificate ("Advanced: use as a TLS client
certificate"), whose ECDSA P-256 key is `ssh -i hams-member-certificate.pem root@router` (`ssh-keygen -y -f` prints the public key). The
signed router configuration lists the owner's active exported keys (`ssh_keys`); `hams-node-enroll` stores them in `STATE/ssh_keys` and
`hams-node-ssh apply` keeps them in a marked block of `/etc/dropbear/authorized_keys` (other lines stay). A certificate on the signed deprecation
list (`cert:ISSUER:SERIAL`) is left out at once (`hams-node-status` re-runs `hams-node-ssh apply`), an expired or revoked one drops out at the next
configuration fetch. When the last key goes, SSH refuses every login and password login STAYS off (recovery is a physical reset, then enrol again). The browser's own key is non-extractable and cannot be used.

## Key rotation (`hams-node-key`, in the hams-node-enroll package)

`hams-node-key rotate` makes a new WireGuard key on the device (`wg genkey` into `wg.key.new`, mode 0600), sends only the new public key with the device
token to `/api/ipv6/enroll/rekey`, and only after hams.com accepts it replaces `/etc/hams-node/wg.key`, loads it into the running tunnel
(`wg set <interface> private-key`, the interface from `hams_node_ondemand.main.interface`), and has `hams-node-endpoint` republish the endpoint
signed with the new key. A refusal or an unreachable hams.com keeps the old key (exit 1 or 4). Peers that hold the old public key learn the new
one from the directory on their next lookup.

## Remote switch-off and rogue-router warnings (`hams-node-status`, separate package)

A compromised router cannot be trusted to switch itself off, so staff cut it on hams.com first (emergency disable); this package is the
second layer. `hams-node-status start` (run by enrolment) adds a cron line every 2 minutes that runs `hams-node-status run`: it posts the
device token to `/api/ipv6/status` and verifies the signed reply with `usign` against `/etc/hams-node/deprecation.pub` (the key hams.com
sends at enrolment). Only a correctly signed `disabled` turns the node off: WireGuard interface down, its routes flushed, bird, on-demand,
report and traffic stopped, configuration kept, the reason saved in `/etc/hams-node/disabled` (LuCI shows it). A later signed `ok` turns
it back on. Fail open: no network, DNS or TLS failure, a missing or bad signature or any other reply changes nothing.

The status also carries the sequence number of the signed deprecation list (keys and prefixes of rogue routers). When it is ahead of the
held list (`deprecation_seq`, `deprecation_list`) the router asks for the delta since the held number, additions and removals, or for the
full list when the delta does not line up; documents are ordered by sequence number only, never by clock, and replayed, out-of-order or
wrongly signed ones are refused. Each listed peer is evicted and each listed prefix gets an `unreachable` route. Format and endpoints:
`docs/proposals/IPV6_DEPRECATION_LIST.md` in hams_com.

Gossip (works with hams.com unreachable): the init script `hams-node-status` runs `hams-node-status listen` (busybox `nc` on tcp 5710, opened in
`hams_node_fw`; it is not an encrypted service, so it may also face RF links). A neighbour sends `HAMS-GOSSIP SEQ`; when the held list is newer
the router answers with the stored chain (`deprecation_chain`: the last full list and the deltas after it, exactly as hams.com signed them,
one per line). The cron line `hams-node-status gossip` (every 2 minutes, with `run`) asks up to 8 neighbours (the on-demand peers' routers at
the first address of their /60, plus any addresses in `/etc/hams-node/gossip_neighbours`, which RF-link setup may fill) and applies each
returned document only if the signature verifies and it lines up (full: not older; delta: `from` equals the held number); forged, replayed and
gapped documents are refused. Nothing but signed documents is relayed, and a router that is up to date is sent nothing. The documents are
authenticated, not secret, so nothing is hidden on RF.

## Whole package on stock AREDN 4.26.10.0, and .ham names through AREDN's dnsmasq (2026-11-23)

QEMU VM from the unmodified official x86-64 image (recipe above). The apks: the `hams-node`, `-enroll`, `-dns` and `-report` packages built with
the 25.12.5 SDK plus the nine Jool apks built from AREDN's own tree (`openwrt/aredn-jool`). `apk add --allow-untrusted --no-network` of all of them
resolved with no AREDN or OpenWrt feed (the other subpackages need nginx-ssl, unbound, odhcpd, openssl-util, which stock AREDN lacks; they belong
to the stock OpenWrt router form). Results:

- `/etc/init.d/hams-node start` exits 0 ("active with no instances"); `jool`, `jool_siit`, `jool_common` are loaded; `jool_siit instance display` works.
- babeld, arednlink, uhttpd, dropbear and dnsmasq stayed running through install, start, and `hams-node-dns` start/stop.
- `hams-node-dns` (`enabled 1`, one resolver) writes `server=/ham/ADDR` into `/tmp/dnsmasq.d`, which AREDN's dnsmasq reads (`conf-dir`), and
  sets `rebind_domain ham`. Queried through the node's own dnsmasq (a stub .ham resolver behind it): with the snippet a .ham A name and a PTR are
  answered through the node and the same name is REFUSED without it; a `name.local.mesh` host from the arednlink hosts directory resolves the
  same before and after.
- A snippet that dnsmasq rejects stops AREDN's dnsmasq entirely (found by accident with a duplicated `log-facility`): only the renderer's own lines
  go into that directory.
- **AREDN's dnsmasq has `filter-AAAA`** (`/etc/dnsmasq.conf`): every AAAA answer, also a forwarded one, is dropped (A answered, AAAA `answers=0`;
  a plain dnsmasq with `--filter-AAAA` and the same forward behaves the same). So through the node's resolver the SIIT AAAA of a .ham name is never
  seen. IPv6 clients must ask the hams.com Net resolvers directly (second gateway form: the stock OpenWrt router's dnsmasq has no such filter).
  The forward still serves the non-AAAA types (service-ad CNAME targets, TXT, PTR). The filter is firmware configuration and is not changed here.
- **AAAA for IPv6 clients (final rule, tested 2026-11-23).** The filter is not only a setting: a second dnsmasq of AREDN's own binary (no
  `filter-AAAA`, `-C /dev/null`) also answers every forwarded AAAA with NODATA (`config ... is NODATA` in its log; a local `host-record` AAAA is
  answered), and that build has no DHCPv6/RA. So `hams-node-dns` offers `option v6_addr ADDR` (and `v6_iface`, default `br-lan`): the init script
  adds ADDR/128 to the interface and an nft nat table `hams_dns6` sends port 53 (udp and tcp) of ADDR to the first IPv6 resolver of the `resolver`
  list (`ADDR#PORT` allowed), with masquerade. No process runs; AREDN's dnsmasq is untouched (the DNAT precedes local delivery, so its own socket on
  that address never sees the queries; the node's own address still answers without AAAA). IPv6 clients get ADDR as resolver from whatever sends
  their router advertisements (RDNSS) or DHCPv6; all their queries reach that resolver. The firewall zone toward the resolver must allow the
  forward (AREDN's fw4 only forwards lan to its wifi zone; the test added one accept rule).
  Tested on stock 4.26.10.0 x86-64, two QEMU VMs (client and node on one segment, a stub resolver on the host): from the client `AAAA foo.n0call.ham`
  at ADDR returned `2001:db8:5::1`, A returned the A record, the same AAAA at the node's own address stayed empty, `stop` removed the table and AREDN's
  dnsmasq kept running. Router advertisement of ADDR is not tested.
- **AAAA for IPv6 clients (final rule, tested 2026-11-23).** The filter is not a setting of our snippet: a second dnsmasq of AREDN's own binary
  (no `filter-AAAA` in its config, `-C /dev/null`) also answers every forwarded AAAA with NODATA (`config ... is NODATA` in its log; a local
  `host-record` AAAA is answered), and that build has no DHCPv6/RA. So `hams-node-dns` offers `option v6_addr ADDR` (and `v6_iface`, default
  `br-lan`): the init script adds ADDR/128 to the interface and an nft nat table `hams_dns6` sends port 53 (udp and tcp) of ADDR to the first
  IPv6 resolver of the `resolver` list (`ADDR#PORT` allowed), with masquerade. No process runs; AREDN's dnsmasq is untouched (the DNAT precedes
  local delivery, so its own socket on that address never sees the queries; the node's own address still answers without AAAA). IPv6 clients get
  ADDR as their resolver from whatever sends their router advertisements (RDNSS) or DHCPv6; the client's other queries reach that resolver too.
  Forwarding toward the resolver must be allowed by the firewall zone it is reached through (AREDN's fw4 only forwards lan to its wifi zone).
  Tested on the stock 4.26.10.0 x86-64 image, two QEMU VMs (client and node on one segment, a stub resolver on the host): from the client
  `AAAA foo.n0call.ham` at ADDR returned `2001:db8:5::1` and A returned the A record; the same AAAA at the node's own address stays empty;
  `stop` removes the table and AREDN's dnsmasq kept running.

## hams-node-proxyca: the browser side of the proxy

`hams-node-proxyca ca` makes the node's own CA (EC P-256, key `/etc/hams-node/proxy/ca.key` mode 600, never read by the LuCI plugin or any download),
name-constrained to `.ham`, the whitelisted domains (`/etc/hams-node/whitelist`, same format as ham_proxy's) and the ham network's address block
(`net_cidr`); a certificate it signs for any other name fails `openssl verify` (tested). `hams-node-proxyca pac` writes `/www/hams.pac` and
`/www/wpad.dat`: `.ham` names, ham-network addresses (`net_prefix`) and whitelisted sites get `PROXY`, everything else `DIRECT` (tested by evaluating the
PAC in node). The LuCI page offers "Download CA certificate" (the `proxy_ca` ubus method returns the certificate only); `INSTALL_CA.md` has the install steps
for Windows, macOS, Linux, Firefox, Chrome and Android. Changing the whitelist needs `ca --renew` and a new install. Tests: `tests/test_hams_node_proxyca.py`.

## Web access for AREDN users (`hams-node-web` and `ham-proxy`, separate packages)

AREDN is IPv4-only, so an IPv4 client on the mesh cannot reach a `.ham` name directly. `hams-node-web` (off by default, `/etc/config/hams_node_web`) runs
`ham_proxy --gateway` on the node's own private mesh/LAN IPv4 address and makes the node's dnsmasq answer A queries for `.ham` with that address
(`address=/ham/ADDR`, written to `/tmp/dnsmasq.d/hams-web.conf`). The client then connects to the gateway with integrity-only TLS (authenticated, never
encrypted: Part 97), which looks the name up at the Hams.com Net resolver itself (`resolver`, because AREDN's dnsmasq drops AAAA), connects over the
node's IPv6 path (SIIT/WireGuard on the Internet link) and relays the page, adding the signed integrity indicator when `sign_key` is set. Sites that are not `.ham` are
served only if on the hams.com signed whitelist (`whitelist`, `whitelist_key`); everything else is refused before any upstream connection, and each flow is
written to the station-ID log with the callsign.

Limits that protect the operator: `hams-node-web` refuses to start unless the listen address is a private IPv4 address (never 0.0.0.0 or a public one),
a callsign and a resolver are set; `ham_proxy` never connects to a loopback, private, link-local or unique-local address, so no name leads the gateway to the
operator's home LAN; nothing is added to the node's firewall and the mesh-to-WAN setting is not touched. The client needs a ham-aware browser or the relay
(a stock browser cannot speak an integrity-only suite); a plain-HTTPS client is refused by design.

`ham-proxy` is the binary package: `openwrt/ham-proxy/Makefile` builds it with the host's cargo (`--features vendored`) for the node's musl Rust target
using the SDK's cross compiler; the SDK has no rust. Verified 2026-11-23 (25.12.6 SDKs, `--locked`):

    make package/feeds/hams/ham-proxy/compile NO_DEPS=1 HAMS_OPEN_TREE=<repo> RUST_TARGET=x86_64-unknown-linux-musl
    make package/feeds/hams/ham-proxy/compile NO_DEPS=1 HAMS_OPEN_TREE=<repo> RUST_TARGET=mipsel-unknown-linux-musl RUST_BUILD_STD=1

`RUST_BUILD_STD=1` (targets without a prebuilt std, such as the mt7621) uses `rustup toolchain install nightly --component rust-src` and `-Z build-std`
under the build user. The x86-64 apk is 6.2 MB static (depends on `libc`); the ramips-mt7621 apk is 1.9 MB, a PIE binary that needs `libgcc` and
`libatomic` (both installed on stock AREDN). The mt7621 package was built but not run (no device or MIPS emulator here). In an SDK that has not built
`libgcc`/`libatomic` the package's library check fails ("missing dependencies"); give the SDK's `staging_dir/target-*/pkginfo/libgcc.provides`
(`libgcc_s.so.1`) and `libatomic.provides` (`libatomic.so.1`) files.

Boot test on the stock AREDN 4.26.10.0 x86-64 image (QEMU, second NIC at 10.0.2.15 for the client): `apk add --allow-untrusted --no-network` of the
Jool apks, `hams-node`, `hams-node-dns`, `hams-node-web`, `ham-proxy`; `hams_node_web` enabled with `listen_addr 10.0.2.15`, a callsign, the resolver
(a stub answering AAAA at the host) and `ca`; the site was uhttpd with TLS on an address of `lo`. From the host (an IPv4 client, `openssl s_client -tls1_2 -cipher
NULL-SHA256:@SECLEVEL=0`) a request for `gw.ham:8444` returned the page with the integrity banner, `example.org` got no reply (refused before any upstream
connection), a client offering only encrypting suites got a handshake failure, and the station-ID log held `W1AW -> gw.ham NULL-SHA256`. The node's dnsmasq
answered `foo.ham` with 10.0.2.15; AREDN's own services kept running.

`ham-proxy` is the static binary: `openwrt/ham-proxy/Makefile` builds it with the host's cargo (`--features vendored`) for the node's musl Rust target using the
SDK's cross compiler (`make package/ham-proxy/compile RUST_TARGET=x86_64-unknown-linux-musl`); the SDK has no rust, and mips needs a nightly `build-std`
toolchain. The x86-64 musl build was measured earlier (6.1 MB, 4.5 MB RSS). The `ham-proxy` Makefile itself has not been run in an SDK yet; `ham_proxy`
and the shell script are tested (`tests/test_hams_node_web.py`, `daemons/ham_proxy/tests/resolve.rs`).

## IPv4 pool for AREDN clients (`hams-node-pool`, separate package)

Beyond web pages: a small IPv4 pool on the AREDN side that maps chosen `.ham` names to pool addresses (`/etc/config/hams_node_pool`, off by default).
`hams-node-pool add NAME.ham IP6` takes the next free address of `pool4` (a /24 to /30 of the node's private space) and records it in
`/etc/hams-node/pool.map`; `apply` (the init script at boot) adds the address to the AREDN-side interface (the node answers ARP for it; AREDN sets `arp_ignore=1`, so an address on `lo` is never answered on the LAN), adds a Jool SIIT explicit address
mapping to the SIIT instance (`jool_siit -i INSTANCE eamt add IP4 IP6`; the client's own source address maps through the domain's /96 as before), has
dnsmasq answer the name with it (`address=/NAME/IP4`, a longer match than the web gateway's `address=/ham/`), and installs an nft table `inet hams_pool`:
only TCP ports listed in `port` (default 80; unencrypted ports only, because the answers go onto RF, Part 97) from the configured AREDN-side interface
reach a pool address, everything else to the pool is dropped and counted. Targets must start with `net_prefix` (the member /40), so the pool cannot lead to
the member's LAN or the mesh; names must end in `.ham`. `clear` (init stop) removes mapping, addresses, table and dnsmasq line.

Tested (`tests/test_hams_node_pool.py`: allocation, exhaustion, refusals, output with recording stand-ins) and run on the stock AREDN 4.26.10.0 x86-64 VM with the
real Jool, nft and dnsmasq: `apply` showed the mapping in `jool_siit eamt display`, the `hams_pool` table, `gw.ham` answered with the pool address and other
`.ham` names with the gateway's; `/etc/init.d/hams-node-pool stop` removed all of it. Traffic test (2026-11-23, stock AREDN 4.26.10.0 x86-64 under QEMU, user networking; the pool address was reached by QEMU's own host forward, the target was
QEMU's IPv6 host address answering for services on the dev box): an IPv4 client got a page through the pool address on the allowed port; a port not in the
list and an encrypted port (8883) got no answer, the target saw no connection, and the drop counter in `hams_pool` counted them; configuring an encrypted port
(22, 443, 853, 993, 995, 465, 8883, 1194; the list `ham_boundary` uses) is refused at `apply`. The test found two faults, both fixed here: the pool address on `lo`
was never answered for on the LAN (`arp_ignore=1`), and AREDN's firewall (fw4) drops forwarded traffic from an interface that is in no zone, so every
translated flow was dropped. `apply` now writes `/etc/hams-node/pool-fw.nft` (the translated requests from the AREDN-side interface to the mapped targets on the
listed ports, and the replies from the pool addresses on those ports) and registers it as an fw4 include at the head of the forward chain; `clear` removes it.
The same drop affects the web gateway's own path only if its clients are routed through the node (they connect to the node itself, which is input, not forward).
The test needed an address map entry for the client (its translated source had to be an address the QEMU network accepts); real traffic uses the domain's /96.
remote target over WireGuard were not available in the VM).
