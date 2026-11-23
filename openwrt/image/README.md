# hams-node x86-64 test image and smoke test

`build_test_image.sh -a APK_DIR -w WORKDIR` downloads the official OpenWrt 25.12.5 x86-64 Image Builder (sha256 checked), builds
a throwaway-key feed from the `.apk` files in APK_DIR with `../feed/build_feed.sh` (plus empty stand-ins for `jool-tools-siit` and
`kmod-jool-siit`, which the official feeds lack), and makes `WORKDIR/hams-test.img` with hams-node, luci-app-hams-node and LuCI.
The image trusts the throwaway key through `/etc/apk/keys`. No root; about 10 minutes the first time. Nothing is published.

`tools/smoke_test.py IMAGE` boots it in QEMU (KVM if available; the disk is a snapshot, nothing is written back), and over the
serial console checks: packages installed, `hams-node` starts and is enabled, uhttpd runs, `ubus call hams-node status`
answers, LuCI answers (login page, HTTP 403) and serves the hams-node view. Exit 0 = pass. Run as an agent_procs job:

    agent_procs.py run owrt-smoke --timeout 600 --mem-max 4G -- python3 -I openwrt/image/tools/smoke_test.py WORKDIR/hams-test.img

Build the .apk files first with the SDK (`../hams-node/README.md`; the luci app builds in an SDK that has only the `hams` feed).
Not exercised: Jool SIIT itself (stand-ins), real enrolment, WireGuard peers. Tests: `python3 -m unittest discover -s openwrt/image/tests`.

`tools/lan_test.py IMAGE` (stock or test image, QEMU, no root): copies `hams-node-lan` in over the console, applies `2001:db8:ab40:10::/60`
and checks that netifd gives br-lan `2001:db8:ab40:10::1/64`, odhcpd runs, and `remove` takes the /64 out of netifd's interface status
(the kernel address lingers a while). Verified on OpenWrt 25.12.5 x86-64. It does not check a client's address (one guest only).

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
