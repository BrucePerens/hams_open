#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Build the hams_node apk feed: copy the .apk files, make packages.adb with `apk mkndx`, sign it with an
# EC prime256v1 key (the OpenWrt 25.12 convention: the SDK's own private-key.pem has this form), and
# publish the matching public key. Usage: build_feed.sh -k PRIVATE_KEY -o OUTDIR APK_DIR...
# APK (env) names the apk binary; default is `apk` on PATH (the SDK has one in staging_dir/host/bin).
# The private key must live outside this repository; see README.md for where the real one belongs.
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
repo=$(cd "$here/../.." && pwd -P)
key= out=
while getopts k:o: opt; do
	case $opt in
	k) key=$OPTARG ;;
	o) out=$OPTARG ;;
	*) exit 2 ;;
	esac
done
shift $((OPTIND - 1))
[ -n "$key" ] && [ -n "$out" ] && [ $# -ge 1 ] || { echo "usage: $0 -k PRIVATE_KEY -o OUTDIR APK_DIR..." >&2; exit 2; }
[ -f "$key" ] || { echo "no such key: $key" >&2; exit 2; }
keyabs=$(cd "$(dirname "$key")" && pwd -P)/$(basename "$key")
case $keyabs in
"$repo"/*) echo "refusing: the signing key must not be inside the repository" >&2; exit 2 ;;
esac
apk=${APK:-apk}
mkdir -p "$out"
for d in "$@"; do cp "$d"/*.apk "$out"/; done
(cd "$out" && "$apk" mkndx --allow-untrusted --sign "$keyabs" --output packages.adb ./*.apk)
openssl ec -in "$keyabs" -pubout > "$out/hams-feed-public.pem" 2>/dev/null
echo "feed written to $out"
