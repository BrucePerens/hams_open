# Cloudflared Daemon

This directory contains the upstream `cloudflared` client source, responsible for proxying traffic between the Cloudflare network and local origins without requiring open firewall ports.

### Context within `hams_open`
While this directory contains the standard Cloudflare Tunnel source, it is packaged and managed here to ensure a controlled and reliable version is compiled and deployed within the `hams_open` environment for secure tunnel routing.

### Building and deploying (production, `hams1`)
`deploy_to_production.py` copies this source tree to `/opt/hams/src/hams_open/daemons/cloudflared/`
but never builds it, and `hams1` has no Go toolchain. The running binary is whatever was last built
by hand, so a vendor bump is not live until someone rebuilds and swaps it.

1. Build on the dev box (same x86_64 architecture as `hams1`), from a checkout at the commit being
   deployed: `daemons/cloudflared/build_hams.sh /some/path/cloudflared`. It builds offline from
   `vendor/` and takes the version from the first line of `RELEASE_NOTES` (the upstream Makefile
   uses `git describe`, which prints an empty version here because the repository has no tags).
   It must end by printing `cloudflared version <YYYY.M.P> (built ...)`.
2. Copy it to `hams1` next to the running binary under a new name, keep the old binary for
   rollback, `mv` the new one into place, then `systemctl restart` the tunnel unit. Restarting drops
   every visitor connection and websocket for a few seconds, and this tunnel is production's only web
   front end, so confirm `curl -sI https://hams.com/` returns 200 and `wss://hams.com/ws/firehose`
   upgrades right away, and move the old binary back if either fails.
3. Verify with `cloudflared --version` on `hams1`.
