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
