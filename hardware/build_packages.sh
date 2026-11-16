#!/usr/bin/env bash
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# Builds the Debian binary and source packages of every hardware/<dir>/debian in a CLEAN copy and collects the
# .deb, .dsc and .tar.xz files (plus SHA256SUMS) in one output directory, ready for the hams.com APT repository
# publisher (hams_com .github/workflows/publish_apt_packages.sh).
#
# Usage: hardware/build_packages.sh <output-dir> [hardware-dir-name...]
#   With no names, every hardware/<dir> that has a debian/ directory is built. Environment:
#   LINTIAN=1   also run `lintian --pedantic` on each result and fail on any tag (lintian must be installed).
# The copy is made from the files git TRACKS (`git ls-files`), never from the working directory: an untracked or
# git-ignored file (for example a *.patch, which .gitignore excludes) is absent from the copy, so a package that only
# builds on a machine with leftovers fails here, the same as it would for anyone else.
# Builds are `Architecture: all` packages, so one build serves every architecture; native 3.0 format sources.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ "$#" -ge 1 ] || { echo "usage: $0 <output-dir> [hardware-dir-name...]" >&2; exit 2; }
OUT="$(mkdir -p "$1" && cd "$1" && pwd)"
shift
if [ "$#" -gt 0 ]; then
  DIRS=("$@")
else
  DIRS=()
  for d in "$HERE"/*/debian; do [ -d "$d" ] && DIRS+=("$(basename "$(dirname "$d")")"); done
fi
[ "${#DIRS[@]}" -gt 0 ] || { echo "ERROR: no hardware/<dir>/debian found" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

for NAME in "${DIRS[@]}"; do
  SRC="$HERE/$NAME"
  [ -d "$SRC/debian" ] || { echo "ERROR: $NAME has no debian/ directory" >&2; exit 1; }
  COPY="$WORK/$NAME"
  mkdir -p "$COPY"
  ( cd "$SRC" && git ls-files -z -- . | xargs -0 -I{} cp --parents -p {} "$COPY/" )
  # The source package name and version come from debian/changelog; dpkg-buildpackage puts its output in the parent.
  ( cd "$COPY" && nice -n 10 dpkg-buildpackage -us -uc -b >"$WORK/$NAME.build.log" 2>&1 \
      && nice -n 10 dpkg-buildpackage -us -uc -S -d >>"$WORK/$NAME.build.log" 2>&1 ) \
    || { echo "ERROR: building $NAME failed; log follows" >&2; cat "$WORK/$NAME.build.log" >&2; exit 1; }
  if [ -n "${LINTIAN:-}" ]; then
    shopt -s nullglob
    for F in "$WORK"/*.changes; do lintian --pedantic --fail-on error,warning,info,pedantic "$F" || { echo "ERROR: lintian found tags in $F" >&2; exit 1; }; done
    shopt -u nullglob
  fi
  shopt -s nullglob
  for F in "$WORK"/*.deb "$WORK"/*.dsc "$WORK"/*.tar.xz; do mv -f "$F" "$OUT/"; done
  rm -f "$WORK"/*.changes "$WORK"/*.buildinfo
  shopt -u nullglob
  echo "built $NAME"
done

( cd "$OUT" && sha256sum -- *.deb *.dsc *.tar.xz > SHA256SUMS )
echo "wrote $(grep -c . "$OUT/SHA256SUMS") files and $OUT/SHA256SUMS"
