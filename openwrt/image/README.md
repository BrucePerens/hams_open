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
