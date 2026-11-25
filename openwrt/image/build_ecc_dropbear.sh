#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Builds OpenWrt's own dropbear package with ECDSA (P-256) turned on, as an apk for the hams feed. Stock OpenWrt builds dropbear without ECDSA, so the
# ECDSA P-256 member-certificate keys that hams-node-ssh installs cannot log in (to-do router-ssh-ecdsa-member-key). There is no ECC dropbear variant
# in the OpenWrt package feeds (checked 25.12.5: only openssh-server, a much bigger package).
# Usage: build_ecc_dropbear.sh -s SDK_DIR -o OUT_DIR
#   SDK_DIR: the unpacked OpenWrt SDK of the node's target (x86-64 for the QEMU image; AREDN/other routers need their own target's SDK).
#   OUT_DIR: gets dropbear-<version>-r<N>.apk; put it next to the hams-node apks (feed/build_feed.sh) or into build_test_image.sh -d.
# The release number is the stock one plus 1000, so apk prefers this build over the stock package of the same version (a stock security update
# to a new dropbear VERSION still wins; rebuild then). Only the base feed is fetched. No root needed.
set -eu
sdk= out=
while getopts s:o: opt; do
	case $opt in
	s) sdk=$OPTARG ;;
	o) out=$OPTARG ;;
	*) exit 2 ;;
	esac
done
[ -n "$sdk" ] && [ -d "$sdk" ] && [ -n "$out" ] || { echo "usage: $0 -s SDK_DIR -o OUT_DIR" >&2; exit 2; }
mkdir -p "$out"
cd "$sdk"
if [ ! -d package/feeds/base/dropbear ]; then
	grep -E '^src-git --root=package base ' feeds.conf.default > feeds.conf
	./scripts/feeds update base
	./scripts/feeds install -p base dropbear
fi
make defconfig >/dev/null
printf 'CONFIG_DROPBEAR_ECDSA=y\nCONFIG_DROPBEAR_ECC_256=y\n' >> .config
make defconfig >/dev/null
grep -q '^CONFIG_DROPBEAR_ECDSA=y' .config || { echo "ECDSA option not set" >&2; exit 1; }
stock=$(sed -n 's/^PKG_RELEASE:=//p' package/feeds/base/dropbear/Makefile | head -1)
make package/feeds/base/dropbear/compile PKG_RELEASE=$((stock + 1000)) "-j$(nproc)"
cp bin/targets/*/*/packages/dropbear-[0-9]*.apk "$out/"
ls "$out"/dropbear-*.apk
