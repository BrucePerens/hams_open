#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Reproducible x86-64 test image: official OpenWrt Image Builder + our signed feed (../feed) with a throwaway key.
# Usage: build_test_image.sh -a APK_DIR -w WORKDIR     (APK_DIR holds the hams-node and luci-app-hams-node .apk files)
# Result: WORKDIR/hams-test.img (raw, boots with QEMU; see smoke_test.py). No root needed. Nothing is published.
# The official feeds have no jool-tools-siit / kmod-jool-siit, which hams-node depends on; empty stand-ins are
# built into the same test feed so the apk dependency check passes (the SIIT step itself is not exercised).
set -eu
VERSION=${OPENWRT_VERSION:-25.12.5}
here=$(cd "$(dirname "$0")" && pwd -P)
apkdir= work=
while getopts a:w: opt; do
	case $opt in
	a) apkdir=$OPTARG ;;
	w) work=$OPTARG ;;
	*) exit 2 ;;
	esac
done
[ -n "$apkdir" ] && [ -n "$work" ] || { echo "usage: $0 -a APK_DIR -w WORKDIR" >&2; exit 2; }
mkdir -p "$work"
work=$(cd "$work" && pwd -P)
base=https://downloads.openwrt.org/releases/$VERSION/targets/x86/64
tarball=openwrt-imagebuilder-$VERSION-x86-64.Linux-x86_64.tar.zst

if [ ! -f "$work/$tarball" ]; then
	curl -sSfo "$work/$tarball" "$base/$tarball"
	curl -sSf "$base/sha256sums" | grep -F "$tarball" | sed "s|\*.*|$work/$tarball|" | sed 's|  | |' > "$work/sha.txt"
	echo "$(cut -d' ' -f1 "$work/sha.txt")  $work/$tarball" | sha256sum -c -
fi
rm -rf "$work/ib" "$work/feed"
mkdir "$work/ib"
zstd -dc "$work/$tarball" | tar x -C "$work/ib" --strip-components=1

# the apk binary comes with the Image Builder
APK=$work/ib/staging_dir/host/bin/apk
export APK
[ -x "$APK" ] || APK=$(ls "$work"/ib/staging_dir/host/bin/apk)

# throwaway key and the stand-in packages
[ -f "$work/test-feed-private.pem" ] || openssl ecparam -name prime256v1 -genkey -noout -out "$work/test-feed-private.pem"
rm -rf "$work/stubs" "$work/stubsrc"
mkdir -p "$work/stubs" "$work/stubsrc"
for n in jool-tools-siit kmod-jool-siit; do
	"$APK" mkpkg --info "name:$n" --info "version:0-r1" --info "arch:x86_64" --info "description:test stand-in" \
		--info "license:GPL-2.0-or-later" --files "$work/stubsrc" --output "$work/stubs/$n-0-r1.apk"
done
sh "$here/../feed/build_feed.sh" -k "$work/test-feed-private.pem" -o "$work/feed" "$apkdir" "$work/stubs"

# trust the key and use the feed, both while building and in the image
cp "$work/feed/hams-feed-public.pem" "$work/ib/keys/"
echo "file://$work/feed/packages.adb" >> "$work/ib/repositories"
mkdir -p "$work/files/etc/apk/keys"
cp "$work/feed/hams-feed-public.pem" "$work/files/etc/apk/keys/"

PACKAGES="hams-node luci-app-hams-node luci wireguard-tools kmod-wireguard jsonfilter uclient-fetch"
make -C "$work/ib" image PROFILE=generic PACKAGES="$PACKAGES" FILES="$work/files" ROOTFS_PARTSIZE=256 EXTRA_IMAGE_NAME=hams
gz=$(ls "$work"/ib/bin/targets/x86/64/*-ext4-combined.img.gz 2>/dev/null || ls "$work"/ib/bin/targets/x86/64/*combined*.img.gz | head -1)
gunzip -c "$gz" > "$work/hams-test.img" || true   # gzip warns about trailing garbage in combined images
echo "image: $work/hams-test.img"
