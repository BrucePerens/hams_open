#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-or-later
# Build Jool (kmod-jool-netfilter, jool-tools-netfilter, libnl, kmod-crypto-md5) that loads on the STOCK pre-built
# AREDN firmware of one target, by building from AREDN's own release tree. Nothing is published.
# Usage: build_aredn_jool.sh -t TARGET [-r RELEASE] [-w WORKDIR] [-o OUTDIR] [-j JOBS] [-k]
#   TARGET   AREDN config name: x86-64 ath79-generic ath79-mikrotik ath79-nand ipq40xx-generic ipq40xx-mikrotik
#            mediatek-filogic ramips-mt7621 ramips-mt76x8   (see SUPPORTED_DEVICES.md in the AREDN tree)
#   RELEASE  AREDN tag, default 4.26.10.0     OUTDIR  default ./out/TARGET     -k  keep the build tree
# Steps: fetch the tag, add only the Jool package options, build the kernel, then VERIFY the kernel config on struct-layout options (NET_NS etc.) and that the tree's kernel modules are
# ABI-identical to the release's published ones (vermagic + every modversion CRC), then build Jool.
# Only if the verification passes is the release image's kernel hash (read from AREDN's published package list) written
# into the tree's .vermagic so the kmod's `kernel` dependency matches the stock image. Otherwise: exit 3, nothing built.
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
rel=4.26.10.0 target="" work="" out="" jobs=$(nproc) keep=0
while getopts t:r:w:o:j:k opt; do
	case $opt in
	t) target=$OPTARG ;; r) rel=$OPTARG ;; w) work=$OPTARG ;; o) out=$OPTARG ;; j) jobs=$OPTARG ;; k) keep=1 ;;
	*) exit 2 ;;
	esac
done
[ -n "$target" ] || { echo "usage: $0 -t TARGET [-r RELEASE] [-w WORKDIR] [-o OUTDIR] [-j JOBS] [-k]" >&2; exit 2; }
main=${target%%-*} sub=${target#*-}
case $target in
x86-64) main=x86 sub=64 ;;
ath79-mikrotik-ath10k | ramips-*-morse) echo "unsupported target $target" >&2; exit 2 ;;
esac
major=${rel%%.*} rest=${rel#*.} minor=${rest%%.*}
rbase=https://downloads.arednmesh.org/releases/$major/$minor/$rel
tbase=$rbase/targets/$main/$sub
work=${work:-$PWD/work}/$target
out=${out:-$PWD/out}/$target
dl=$(dirname "$work")/dl
mkdir -p "$work" "$out" "$dl"
log() { echo "== $(date +%H:%M:%S) $*"; }

# 1. the release's published kernel hash (the kernel apk's name) and kernel version
curl -sSf --max-time 120 "$tbase/packages/" -o "$work/pkgindex.html"
kapk=$(grep -o 'href="kernel-[^"]*\.apk"' "$work/pkgindex.html" | head -1 | cut -d'"' -f2)
[ -n "$kapk" ] || { echo "cannot read the release kernel package from $tbase/packages/" >&2; exit 3; }
relhash=$(echo "$kapk" | sed -n 's/^kernel-[0-9.]*~\([0-9a-f]\{32\}\)-r[0-9]*\.apk$/\1/p')
kver=$(echo "$kapk" | sed -n 's/^kernel-\([0-9.]*\)~.*/\1/p')
[ -n "$relhash" ] || { echo "cannot parse kernel hash from $kapk" >&2; exit 3; }
log "release $rel $target: kernel $kver hash $relhash"

# 2. AREDN tree at the release tag
if [ ! -d "$work/aredn/.git" ]; then
	git clone -q --depth 1 --branch "$rel" https://github.com/aredn/aredn.git "$work/aredn"
fi
cd "$work/aredn"
[ "$(git rev-parse HEAD)" = "$(git rev-parse "refs/tags/$rel^{commit}")" ] || { echo "tree is not at tag $rel" >&2; exit 3; }
sed -i "s/^MAKE_ARGS=.*/MAKE_ARGS=-j$jobs/" config.mk
make TARGET="$target" prepare > "$work/prepare.log" 2>&1 || { tail -20 "$work/prepare.log" >&2; exit 1; }
cd openwrt
rm -rf dl && ln -s "$dl" dl
./scripts/feeds install jool libnl >> "$work/prepare.log" 2>&1
# the only change to AREDN's configuration: build the Jool packages as modules
cat >> .config <<CFG
CONFIG_PACKAGE_kmod-jool-netfilter=m
CONFIG_PACKAGE_jool-tools-netfilter=m
CONFIG_PACKAGE_kmod-crypto-md5=m
CONFIG_PACKAGE_libnl=m
CFG
make defconfig >> "$work/prepare.log" 2>&1
grep -q '^CONFIG_PACKAGE_kmod-jool-netfilter=m' .config || { echo "jool not selectable in this tree" >&2; exit 1; }

# 3. kernel and the modules the release also ships
log "kernel build"
make -j"$jobs" tools/install toolchain/install target/linux/compile package/kernel/linux/compile > "$work/kernel.log" 2>&1 \
	|| { tail -30 "$work/kernel.log" >&2; exit 1; }
linuxdir=$(ls -d build_dir/target-*/linux-*/linux-"$kver")
apk=staging_dir/host/bin/apk

# 4a. kernel config vs AREDN's published config on struct-layout options (NET_NS: see check_kconfig.py)
curl -sSf --max-time 120 "$tbase/config.buildinfo" -o "$work/config.buildinfo" || { echo "cannot fetch published config.buildinfo" >&2; exit 3; }
python3 -I "$here/check_kconfig.py" "$work/config.buildinfo" .config "$linuxdir/.config" || { echo "kernel config differs from the release on layout options: refusing" >&2; exit 3; }

# 4b. verify ABI identity with the release: same-named kernel modules must have identical vermagic and modversion CRCs
log "verify against release modules"
vdir=$work/relmods
rm -rf "$vdir" && mkdir -p "$vdir"
for m in kmod-nft-core kmod-nf-conntrack kmod-nf-nat kmod-nf-ipt kmod-crypto-md5 kmod-udptunnel4 kmod-ip6-tunnel; do
	f=$(grep -o "href=\"$m-[0-9][^\"]*\.apk\"" "$work/pkgindex.html" | head -1 | cut -d'"' -f2 || true)
	[ -n "$f" ] || continue
	curl -sSf --max-time 300 "$tbase/packages/$f" -o "$vdir/$f" || continue
	mkdir -p "$vdir/x/$m" && "$apk" extract --allow-untrusted --destination "$vdir/x/$m" "$vdir/$f" >/dev/null 2>&1 || true
done
python3 -I "$here/compare_modabi.py" "$vdir/x" "$linuxdir" --min-modules 3 || { echo "ABI verification FAILED for $target: refusing" >&2; exit 3; }
mhash=$(cat "$linuxdir/.vermagic")
if [ "$mhash" != "$relhash" ]; then
	log "tree kernel hash $mhash differs from release $relhash only by package selection; ABI verified, writing release hash"
	echo "$relhash" > "$linuxdir/.vermagic"
fi
make package/kernel/linux/compile >> "$work/kernel.log" 2>&1

# 5. Jool and libnl
log "jool build"
make -j"$jobs" package/libnl/compile package/jool/compile > "$work/jool.log" 2>&1 || { tail -30 "$work/jool.log" >&2; exit 1; }
ls bin/targets/*/*/packages/kmod-jool-netfilter-*.apk >/dev/null
find bin -name 'kmod-jool-netfilter-*.apk' -o -name 'kmod-crypto-md5-*.apk' > "$work/apklist"
find bin -name 'jool-tools-netfilter-*.apk' -o -name 'libnl*.apk' >> "$work/apklist"
while read -r f; do cp "$f" "$out/"; done < "$work/apklist"
# the built kmod must now depend on the release kernel
"$apk" adbdump "$out"/kmod-jool-netfilter-*.apk 2>/dev/null | grep -q "kernel.*$relhash" \
	|| { echo "kmod does not depend on kernel ~$relhash: refusing" >&2; exit 3; }
echo "$rel $target kernel $kver~$relhash ABI-verified" > "$out/VERIFIED"
log "done: $out"
[ "$keep" = 1 ] || { cd "$out" && rm -rf "$(dirname "$work")/$target"; }
