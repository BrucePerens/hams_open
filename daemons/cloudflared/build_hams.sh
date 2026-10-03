#!/usr/bin/env bash
# Build the vendored cloudflared for hams.com's production host (linux/amd64), offline from vendor/.
#
# Why this script exists: the upstream Makefile takes the version from `git describe` on a tag, and
# this repository carries no such tags, so a plain `make cloudflared` prints an empty version string
# (production's binary did, until rebuilt with this script). The version here is the first line of
# RELEASE_NOTES, which is what upstream's own tag was cut from.
#
# Usage:  ./build_hams.sh [output-path]      (default: ./cloudflared, which is git-ignored)
# Needs a Go toolchain matching go.mod; the production host has none, so build on the dev box
# (same CPU architecture, x86_64) and copy the binary over. See README.md "Building and deploying".
set -euo pipefail
cd "$(dirname "$0")"
out="${1:-./cloudflared}"
version="$(head -n 1 RELEASE_NOTES | tr -d '[:space:]')"
if ! [[ "$version" =~ ^[0-9]{4}\.[0-9]+\.[0-9]+$ ]]; then
    echo "RELEASE_NOTES does not start with a YYYY.M.P version (got '$version')" >&2
    exit 1
fi
built="$(date -u -r RELEASE_NOTES '+%Y-%m-%d-%H:%M UTC')"
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -mod=vendor -trimpath \
    -ldflags "-X \"main.Version=${version}\" -X \"main.BuildTime=${built}\"" \
    -o "$out" ./cmd/cloudflared
"$out" --version
