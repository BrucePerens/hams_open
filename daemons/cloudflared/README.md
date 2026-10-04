# Cloudflared Daemon

This directory contains the upstream `cloudflared` client source, responsible for proxying traffic between the Cloudflare network and local origins without requiring open firewall ports.

### Context within `hams_open`
While this directory contains the standard Cloudflare Tunnel source, it is packaged and managed here to ensure a controlled and reliable version is compiled and deployed within the `hams_open` environment for secure tunnel routing.

## Building the production binary and swapping it in

Production (`hams1`, Debian 13, x86_64, glibc 2.41, the same as the dev box) runs
`/opt/hams/src/hams_open/daemons/cloudflared/cloudflared` as the `odoo` user's `systemd --user` unit
`cloudflared@<tunnel id>.service` (rendered from `hams_shared`'s template by `cloudflare/utils/cloudflare_systemd.py`).
`deploy_to_production.py` copies the vendored **source** only. The binary is untracked, so it is rebuilt by hand
after every vendor bump (`vendor/` plus `RELEASE_NOTES` changed). `hams1` has Go (`/usr/local/go`, 1.27.1 on
2026-10-03), so the build can run there as the `ai` user in a scratch directory, but building on the dev box is
equivalent because the OS image and glibc match, and it keeps the compile off the production CPU.

Why `cloudflared --version` printed an empty version on 2026-10-03: `main.Version` defaults to `DEV`; the
Makefile fills it from `git describe`, and `/opt/hams/src` has no `.git`, so a build there sets it to the empty
string. Always pass the version explicitly.

### 1. Build (dev box, nothing touches production)

From a `hams_open` checkout at `origin/main`, with the vendor tree present:

```bash
cd daemons/cloudflared
VERSION=$(head -1 RELEASE_NOTES)           # for example 2026.9.3
BUILT=$(date -u -d "$(git log -1 --format=%cI -- RELEASE_NOTES)" '+%Y-%m-%d-%H:%M UTC')
env GOPROXY=off GOFLAGS=-buildvcs=false GOTOOLCHAIN=auto GOOS=linux GOARCH=amd64 \
  nice -n 19 go build -mod=vendor \
  -ldflags="-X \"main.Version=$VERSION\" -X \"main.BuildTime=$BUILT\"" \
  -o /path/to/scratch/cloudflared github.com/cloudflare/cloudflared/cmd/cloudflared
/path/to/scratch/cloudflared --version     # cloudflared version 2026.9.3 (built ...)
sha256sum /path/to/scratch/cloudflared
```

`GOPROXY=off` plus `-mod=vendor` proves nothing is fetched. `go.mod` names the Go version it needs
(`go 1.26.0`); `GOTOOLCHAIN=auto` uses a cached matching toolchain and fails offline if none is cached (then build on
`hams1`, which has a newer one). The build takes about 20 seconds with 4 cores.

The Makefile's `make cloudflared` is the same command, with `VERSION` from `git describe` (needs the upstream tags,
which this repository does not have) and `BuildTime` from the file time of `RELEASE_NOTES`.

### 2. Swap and restart (production change: Bruce, or a session he authorizes)

Keep the old binary for rollback, replace by rename (copying over a running executable fails with "text file
busy"), and restart the one unit. A restart drops every connection and websocket for a few seconds; do it at a
quiet hour.

```bash
# on hams1, as ai; NEW is the file copied over (scp it to /tmp first, check its sha256sum)
D=/opt/hams/src/hams_open/daemons/cloudflared
sudo cp -p $D/cloudflared $D/cloudflared.prev
sudo install -m 0755 -o root -g root /tmp/cloudflared.new $D/cloudflared.new
sudo mv -f $D/cloudflared.new $D/cloudflared
U=$(id -u odoo)
sudo -u odoo env XDG_RUNTIME_DIR=/run/user/$U systemctl --user restart cloudflared@515643a0-6c67-4c8c-9173-519a3a516fa2.service
```

### 3. Verify at once

```bash
$D/cloudflared --version
curl -sI https://hams.com/ | head -1                       # HTTP/2 200
# the firehose websocket must answer 101:
curl -s -o /dev/null -w '%{http_code}\n' --http1.1 -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
  -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' https://hams.com/ws/firehose
sudo -u odoo env XDG_RUNTIME_DIR=/run/user/$U systemctl --user is-active cloudflared@515643a0-6c67-4c8c-9173-519a3a516fa2.service
```

### 4. Roll back

```bash
sudo mv -f $D/cloudflared.prev $D/cloudflared
sudo -u odoo env XDG_RUNTIME_DIR=/run/user/$U systemctl --user restart cloudflared@515643a0-6c67-4c8c-9173-519a3a516fa2.service
```

The unit has `Restart=always`, so a binary that exits at start is retried every 5 seconds and the site stays down
until the rollback: watch `journalctl` for the unit (as `odoo`, `--user`) while checking.
