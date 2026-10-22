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

Use a timestamped backup name instead of a fixed `.prev` (`cloudflared.prev-$(date -u +%Y%m%dT%H%M%SZ)`) so repeated
swaps never overwrite the previous rollback target, and start `sudo pagerduty-maintenance start` first (`end` afterwards).

Two traps found on 2026-10-12 (the dependency bump #592 landed after the 2026-10-04 build, so the running binary still
carried gopsutil 4.26.8 and grpc 1.83.2 although `--version` printed the same `2026.9.3` as the fresh build):

- `--version` cannot tell two builds of the same release apart. Compare `go version -m <binary>` (module versions) and
  `sha256sum /proc/<pid>/exe` against the file you built.
- `deploy_to_production.py --sync-daemons cloudflared` compares and copies `/opt/hams/daemons/cloudflared/`, a separate
  copy that no unit runs. The unit runs `/opt/hams/src/hams_open/daemons/cloudflared/cloudflared`, which the ordinary
  (non-`--sync-daemons`) deploy keeps current. So a clean `--sync-daemons` report says nothing about the running binary.

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

## Upstream release scripts (`make` release targets only)

The three Python scripts in this directory are Cloudflare's own release pipeline, kept because the vendored `Makefile`
calls them (`github_release.py` from `github-release-dryrun` and `github-release`, `github_message.py` from
`github-release`, `release_pkgs.py` from `r2-linux-release` and `r2-next-linux-release`). hams.com does not run them: production is built with `build_hams.sh`
(above). They were reviewed and fixed in the bug hunt of 2026-09-09 (commit `8242d8fa`), and their tests run with
`python3 -m pytest daemons/cloudflared/test_github_message.py daemons/cloudflared/test_github_release.py
daemons/cloudflared/test_release_pkgs.py`. The tests replace PyGithub, boto3, python-gnupg, `reprepro`, `rpm` and
`go version -m` with mocks (`release_script_testing.py` supplies stand-in modules for the packages that are not
installed), so they make no network, GitHub or signing call.

### `github_message.py`: checksums into the release notes

Reads the sha256 of each uploaded binary back out of Workers KV and writes them into the GitHub release's notes.

- `_raise_for_kv_error` [@ANCHOR: github_message:_raise_for_kv_error] raises for any Workers KV answer other than 200,
  using the first entry of the body's `errors` array or else the raw body text, so a failed read can never be mistaken
  for data.
- `kv_get_keys` [@ANCHOR: github_message:kv_get_keys] lists the KV keys that start with a prefix
  (`update_<version>_`), one per uploaded binary.
- `kv_get_value` [@ANCHOR: github_message:kv_get_value] reads one key's value, the checksum text, with the key
  percent-encoded into the URL.
- `update_or_add_message` [@ANCHOR: github_message:update_or_add_message] sets the `name: sha` line for a binary in the
  notes text: it replaces that binary's own line (matched at the start of a line, so `cloudflared-linux-amd64` does not
  match `cloudflared-linux-amd64-fips`), else adds the line inside the existing code block, else starts the
  "SHA256 Checksums" heading and block.
- `get_release` [@ANCHOR: github_message:get_release] returns the GitHub release for a version tag, or `None` when GitHub
  says there is none.
- `parse_args` [@ANCHOR: github_message:parse_args] reads the GitHub key, KV namespace, KV account, KV token and release
  version from flags or environment variables, and prints usage and exits with status 1 if any is missing.
- `main` [@ANCHOR: github_message:main] builds the notes from every KV key of the version and, unless `--dry-run`,
  writes them to the release; a missing release or any error is logged and exits with status 1.

### `github_release.py`: upload binaries to the release

Uploads the built binaries as GitHub release assets and records each one's sha256 in Workers KV.

- `get_sha256` [@ANCHOR: github_release:get_sha256] returns the hex sha256 of a file, read in 4 KiB blocks.
- `send_hash` [@ANCHOR: github_release:send_hash] stores a checksum in Workers KV under `update_<version>_<name>` and
  raises if Workers KV does not answer 200.
- `assert_tag_exists` [@ANCHOR: github_release:assert_tag_exists] raises unless the repository has a git tag exactly
  equal to the version; it searches every tag rather than trusting the list order.
- `get_or_create_release` [@ANCHOR: github_release:get_or_create_release] returns the release for the version tag, and
  otherwise (after requiring the tag to exist) creates it, as a draft if asked; a dry run creates nothing, and a
  "release already exists" conflict from a parallel build is answered by fetching that build's release.
- `parse_args` [@ANCHOR: github_release:parse_args] reads the GitHub key, version, asset path and name, KV settings,
  `--dry-run` and `--draft` from flags or environment variables; the asset name is optional only when the path is a
  directory, and any other missing value prints usage and exits with status 1.
- `upload_asset` [@ANCHOR: github_release:upload_asset] uploads one file unless the release already has an asset of that
  name, then always sends the file's checksum to Workers KV, so a retry after a crash still records it; for a `.tgz` the
  checksum is that of the `cloudflared` binary inside it.
- `move_asset` [@ANCHOR: github_release:move_asset] copies an uploaded file into `./artifacts/` (created if needed),
  where the later packaging step expects it; a file already there is left alone.
- `get_binary_version` [@ANCHOR: github_release:get_binary_version] runs `go version -m` on a binary and returns the
  `main.Version` embedded by the build, raising if the output has no such marker.
- `assert_asset_version` [@ANCHOR: github_release:assert_asset_version] checks that a binary (`.exe`, no extension, or
  the binary inside a `.tgz`) reports the release version and exits with status 1 if it does not; files with any other
  extension are not checked.
- `main` [@ANCHOR: github_release:main] is the entry point: a dry run only checks versions, and a real run checks every
  file's version, finds or creates the release, then uploads each file and copies it to `artifacts`.

### `release_pkgs.py`: signed apt and yum repositories on R2

Builds the `.deb` (reprepro) and `.rpm` (createrepo_c) repositories, signs them, and uploads them to Cloudflare R2.

- `PkgUploader.__init__` [@ANCHOR: release_pkgs:PkgUploader::__init__] keeps the R2 account id, bucket name, client id
  and client secret.
- `PkgUploader.upload_pkg_to_r2` [@ANCHOR: release_pkgs:PkgUploader::upload_pkg_to_r2] uploads one file to the bucket
  through the account's S3-compatible R2 endpoint; an R2 error propagates.
- `PkgCreator.create_distribution_conf` [@ANCHOR: release_pkgs:PkgCreator::create_distribution_conf] writes reprepro's
  `distributions` file, one stanza per Debian/Ubuntu release naming the architectures, component and signing key.
- `PkgCreator.create_deb_pkgs` [@ANCHOR: release_pkgs:PkgCreator::create_deb_pkgs] runs `reprepro includedeb` for one
  release and `.deb`, and raises with the exit code and stderr on failure.
- `PkgCreator.create_rpm_pkgs` [@ANCHOR: release_pkgs:PkgCreator::create_rpm_pkgs] copies and signs the `.rpm` files,
  runs `createrepo_c ./rpm`, and then signs the repository metadata; a `createrepo_c` failure raises before any
  metadata is signed.
- `PkgCreator.create_repo_file` [@ANCHOR: release_pkgs:PkgCreator::create_repo_file] writes `<binary>.repo`, the file
  `yum` users download, pointing at `<baseurl>/rpm` with `gpgcheck=1` and the given public key URL.
- `PkgCreator._sign_rpms` [@ANCHOR: release_pkgs:PkgCreator::_sign_rpms] signs one `.rpm` with `rpm --addsign` using the
  named key, raising on failure.
- `PkgCreator._sign_repomd` [@ANCHOR: release_pkgs:PkgCreator::_sign_repomd] writes an armored detached `gpg` signature
  of `./rpm/repodata/repomd.xml`, raising on failure.
- `PkgCreator._setup_rpm_pkg_directories` [@ANCHOR: release_pkgs:PkgCreator::_setup_rpm_pkg_directories] finds the
  `.rpm` files of each architecture (`aarch64`, `x86_64`, `386`) under the artifacts directory, copies them to
  `./rpm/<arch>/` and signs each copy.
- `PkgCreator.import_gpg_keys` [@ANCHOR: release_pkgs:PkgCreator::import_gpg_keys] imports a base64 private and public
  key into gpg and returns the private key's fingerprint and first user id, raising if nothing was imported or the key
  is not in the secret-key list afterwards.
- `PkgCreator.import_multiple_gpg_keys` [@ANCHOR: release_pkgs:PkgCreator::import_multiple_gpg_keys] imports the primary
  and, for a key rollover, the secondary keypair, returning `(fingerprint, uid)` pairs with the primary first; a
  keypair missing either half is skipped.
- `PkgCreator.import_rpm_key` [@ANCHOR: release_pkgs:PkgCreator::import_rpm_key] writes the decoded public key to
  `pb.key` and runs `rpm --import` so `rpm` can sign with it, raising on failure.
- `upload_from_directories` [@ANCHOR: release_pkgs:upload_from_directories] uploads every file below a directory to R2
  under `[<release>/]<binary>/<path>`, and lets an upload failure stop the run instead of leaving a half-published
  repository.
- `create_deb_packaging` [@ANCHOR: release_pkgs:create_deb_packaging] writes the reprepro configuration (signing with
  both keys during a rollover), includes `./artifacts/<binary>-linux-<arch>.deb` for every release and architecture, then
  uploads `dists` and `pool` as the latest repository and, given a release version, again under that version.
- `create_rpm_packaging` [@ANCHOR: release_pkgs:create_rpm_packaging] builds and signs the `.rpm` repository, uploads the
  `.repo` file and the `rpm` tree as the latest, and with `upload_repo_file` also under the release version.
- `parse_args` [@ANCHOR: release_pkgs:parse_args] reads the R2 bucket, id, secret and account, the binary name, the GPG
  keys, URLs and release tag from flags or environment variables; a missing bucket, id, secret, account or binary name
  prints usage and exits with status 1.
