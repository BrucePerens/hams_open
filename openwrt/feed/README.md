# hams_node apk feed (OpenWrt 25.12)

OpenWrt 25.12 uses apk. A feed is a directory with the `.apk` files, `packages.adb` (the index, signed) and the public
key. Nothing here is published (relay release hold).

Build the packages with the SDK (see `../hams-node/README.md`; also `../luci-app-hams-node`), then:

    openwrt/feed/build_feed.sh -k /path/to/feed-private.pem -o out/hams_node bin/packages/x86_64/hams

`APK=<sdk>/staging_dir/host/bin/apk` if `apk` is not on PATH. The index is signed with an EC prime256v1 key, the same form as
the SDK's own `private-key.pem`; `hams-feed-public.pem` is written next to it. A device trusts the feed when that file is in
`/etc/apk/keys/` and the feed URL is in `/etc/apk/repositories.d/hams.list` (`feeds.conf.example` shows both lines; the
`src-link` line is for building).

Signing key: the real private key is NOT in this repository (`build_feed.sh` refuses a key inside it). Making it
(`openssl ecparam -name prime256v1 -genkey -noout`), keeping it on the build host outside any repo, and deciding the feed
URL are coordinator/Bruce steps. Tests and the test image (`../image/`) make a throwaway key.

Tests: `python3 -m unittest discover -s openwrt/feed/tests` (uses the SDK's apk, or `HAMS_APK`).
