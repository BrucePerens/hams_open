# hams-node x86-64 test image and smoke test

`build_test_image.sh -a APK_DIR -w WORKDIR` downloads the official OpenWrt 25.12.5 x86-64 Image Builder (sha256 checked), builds
a throwaway-key feed from the `.apk` files in APK_DIR with `../feed/build_feed.sh` (plus empty stand-ins for `jool-tools-siit` and
`kmod-jool-siit`, which the official feeds lack), and makes `WORKDIR/hams-test.img` with hams-node, hams-node-bird, hams-node-dns, hams-node-fw, bird2, luci-app-hams-node and LuCI.
The image trusts the throwaway key through `/etc/apk/keys`. No root; about 10 minutes the first time. Nothing is published.

`tools/smoke_test.py IMAGE` boots it in QEMU (KVM if available; the disk is a snapshot, nothing is written back), and over the
serial console checks: packages installed, `hams-node` starts and is enabled, uhttpd runs, `ubus call hams-node status`
answers, LuCI answers (login page, HTTP 403) and serves the hams-node view. Exit 0 = pass. Run as an agent_procs job:

    agent_procs.py run owrt-smoke --timeout 600 --mem-max 4G -- python3 -I openwrt/image/tools/smoke_test.py WORKDIR/hams-test.img

Build the .apk files first with the SDK (`../hams-node/README.md`; the luci app builds in an SDK that has only the `hams` feed).
Node-network checks (uci-driven, throwaway test values): `hams-node-bird` renders, `bird -p` parses and its own instance runs; `hams-node-dns` forwards
`x.ham` to a stub dnsmasq on 127.0.0.2:5353 and public names stay off it; `hams-node-fw` loads the ham zone, a connection from the host through QEMU's
forward is dropped until `tcp:80` is opened in uci; `hams-node-wg` refuses an interface set to rf. With `-n NEWER_APK_DIR` (the same packages built with a higher PKG_RELEASE) the image carries a local signed feed in /root/update-feed; the
smoke test serves it on 127.0.0.1 and checks that `hams-node-update` rolls back on a failing health check and installs the newer version on a pass.
Enrolment: `tools/enroll-stub` (CGI answering `poll` and `unenroll`, served by uhttpd on 127.0.0.1:8098) lets the test enrol with a token, make a WireGuard
interface with a peer, and check that `hams-node-enroll unenroll` revokes at the stub and removes the peer, interface, key, token and settings.
Not exercised: a live route-server session (no /40 yet),
Jool SIIT itself (stand-ins), real enrolment, WireGuard peers. Tests: `python3 -m unittest discover -s openwrt/image/tests`.

`tools/lan_test.py IMAGE` (stock or test image, QEMU, no root): copies `hams-node-lan` in over the console, applies `2001:db8:ab40:10::/60`
and checks that netifd gives br-lan `2001:db8:ab40:10::1/64`, odhcpd runs, and `remove` takes the /64 out of netifd's interface status
(the kernel address lingers a while). Verified on OpenWrt 25.12.5 x86-64. It does not check a client's address (one guest only).

mTLS management test: `build_test_image.sh -a APK_DIR -w WORKDIR -t FIXTURES/overlay` (an `hams-node-mtls-*.apk` in APK_DIR adds it, nginx-ssl and
openssl-util to the image), where `tools/mtls_test.py fixtures FIXTURES` makes a TEST CA, the owner/other/revoked/rogue client certificates
and the overlay; `tools/mtls_test.py run IMAGE FIXTURES` boots it and checks the HTTPS port with `openssl s_client` in the guest.

`tools/endpoint_test.py IMAGE` (stock image, QEMU with a second NIC for `apk add openssl-util`): copies `hams-node-endpoint` in, runs it with a
stand-in `uclient-fetch` that keeps the request body, and checks on the host that the body's MAC equals the portal's formula and that the
private key is not in it. Verified on OpenWrt 25.12.5 x86-64 (busybox ash, `openssl base64`, `hexdump`; the image has no `base64` or `od`).

`tools/internet_gone_test.py IMAGE` (QEMU, no root, needs the host's internet for about a minute): the "internet gone" scenarios of the
disaster-mode to-do. Two guests are joined by a QEMU socket link (eth1, the simulated RF link, nothing encrypted); bird2 and unbound are
installed over the user-mode NIC, which is then cut. With the internet cut it checks that the nodes form a BGP session over RF with no route
server (`hams-node-bird` neighbours) and ping each other, that a cached `.ham` zone resolves (`hams-node-zone`, anycast unreachable), that a
deprecation-list entry still keeps a key out of WireGuard (`hams-node-wg`), and the clock scenarios of `hams-node-time` (no GPS, no internet and
no GPS, spoofed GPS alone, spoofed GPS against a good source). About 6 minutes:

    agent_procs.py run owrt-gone --timeout 1800 --mem-max 6G -- python3 -I -u openwrt/image/tools/internet_gone_test.py WORKDIR/hams-test.img

`tools/ondemand_test.py IMAGE` (stock image, QEMU, no root; the guest needs the host's internet once for `wireguard-tools`, `kmod-wireguard`, `ip-full`): the on-demand
tunnel daemon with real WireGuard peers. Inside the one guest, network namespaces `lan` (a client of the router's /60) and `far` (a member's router with WireGuard) are
joined by veth links; `hams-node-ondemand` and its init script run unchanged (real nft ruleset, `wg`), only the directory lookup is a stand-in `uclient-fetch`. Checks: an rf link
is refused; the first packets from the LAN client to a member /60 make the peer and are delivered through it; a suspended and an unknown /60 create nothing; idle expiry
(1 minute) removes the peer and new traffic makes it again; 256 more members fill the table, which stays at 256; the RSS of all daemon processes stays under 8 MB. About 10 minutes:

    agent_procs.py run owrt-ondemand --timeout 2400 --mem-max 4G -- python3 -I -u openwrt/image/tools/ondemand_test.py WORKDIR/stock-25.12.5.img
