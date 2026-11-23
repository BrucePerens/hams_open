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

Tests: `python3 -m unittest discover -s openwrt/hams-node/tests` (a stand-in `jsonfilter` is used on the dev box).
