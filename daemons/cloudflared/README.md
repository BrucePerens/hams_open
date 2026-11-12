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
   rollback (section 2 below gives the exact commands, including the timestamped backup name), `mv` the new one into place, then `systemctl restart` the tunnel unit. Restarting drops
   every visitor connection and websocket for a few seconds, and this tunnel is production's only web
   front end, so confirm `curl -sI https://hams.com/` returns 200 and `wss://hams.com/ws/firehose`
   upgrades right away, and move the old binary back if either fails.
3. Verify with `cloudflared --version` on `hams1`.

## Building the production binary and swapping it in

Production (`hams1`, Debian 13, x86_64, glibc 2.41, the same as the dev box) runs
`/opt/hams/src/hams_open/daemons/cloudflared/cloudflared` as the `odoo` user's `systemd --user` unit
`cloudflared@<tunnel id>.service` (rendered by `cloudflare/utils/cloudflare_systemd.py` from `daemons/cloudflared-ffi/packaging/cloudflared@.service.template`, whose `ExecStart` is `<binary> tunnel --no-autoupdate run`, with `Restart=always` and `RestartSec=5`).
`deploy_to_production.py` copies the vendored **source** only. The binary is untracked, so it is rebuilt by hand
after every vendor bump (`vendor/` plus `RELEASE_NOTES` changed). Build on the dev box: the OS image and glibc match
`hams1`, and it keeps the compile off the production CPU. `hams1` has Go (`/usr/local/go`, 1.27.1 on 2026-10-03), which
is older than the `go 1.27.3` that `go.mod` has needed since 2026.10.7, so a build there would first have to fetch a
newer toolchain.

Why `cloudflared --version` printed an empty version on 2026-10-03: `main.Version` defaults to `DEV`; the
Makefile fills it from `git describe`, and `/opt/hams/src` has no `.git`, so a build there sets it to the empty
string. Always pass the version explicitly.

### 1. Build (dev box, nothing touches production)

From a `hams_open` checkout at `origin/main`, with the vendor tree present:

```bash
cd daemons/cloudflared
VERSION=$(head -1 RELEASE_NOTES)           # for example 2026.10.7
BUILT=$(date -u -d "$(git log -1 --format=%cI -- RELEASE_NOTES)" '+%Y-%m-%d-%H:%M UTC')
env GOPROXY=off GOFLAGS=-buildvcs=false GOTOOLCHAIN=auto GOOS=linux GOARCH=amd64 \
  nice -n 19 go build -mod=vendor \
  -ldflags="-X \"main.Version=$VERSION\" -X \"main.BuildTime=$BUILT\"" \
  -o /path/to/scratch/cloudflared github.com/cloudflare/cloudflared/cmd/cloudflared
/path/to/scratch/cloudflared --version     # cloudflared version 2026.10.7 (built ...)
sha256sum /path/to/scratch/cloudflared
```

`GOPROXY=off` plus `-mod=vendor` proves nothing is fetched. `go.mod` names the Go version it needs
(`go 1.27.3`); `GOTOOLCHAIN=auto` uses a cached matching toolchain and fails offline if none is cached (fetch it once
with the proxy on: `GOTOOLCHAIN=go1.27.3 go version`; the download is checked against the Go checksum database). The build takes about 20 seconds with 4 cores.

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
(above). They were hardened in commit `8242d8fa`, and their tests run with
`python3 -m pytest daemons/cloudflared/test_github_message.py daemons/cloudflared/test_github_release.py
daemons/cloudflared/test_release_pkgs.py`. The tests replace PyGithub, boto3, python-gnupg, `reprepro`, `rpm` and
`go version -m` with mocks (`release_script_testing.py` supplies stand-in modules for the packages that are not
installed), so they make no network, GitHub or signing call.

### `github_message.py`: checksums into the release notes

Reads the sha256 of each uploaded binary back out of Workers KV (Cloudflare's key-value store) and writes them into the GitHub release's notes. Run as a script from Cloudflare's `github-release` Makefile target; tests: `test_github_message.py`.

#### `_raise_for_kv_error` [@ANCHOR: github_message:_raise_for_kv_error]

Turns any Workers KV HTTP answer other than 200 into an exception, so that a failed read can never be mistaken for data. Both `kv_get_keys` and `kv_get_value` call it straight after their request. Before it existed, a non-200 answer whose JSON body had an empty or missing `errors` array fell through silently and the error text was used as a checksum.

| Argument | Type | Meaning |
|---|---|---|
| `response` | `requests.Response` | the answer to a Workers KV call; only `status_code`, `json()` and `text` are read |
| `action` | str | short verb phrase put in the message, for example `get checksums` |

**Returns:** Nothing when `status_code` is 200.

**Errors:** Raises `Exception("failed to <action>: <detail>")`. The detail is the first element of the body's `errors` array; if the body is not JSON or the array is empty or missing, it is the raw response text.

**Special concerns:** Pure function of the response: no network, no files, no locks, no state. The message may contain whatever Cloudflare returned, so it can appear in CI logs; the bearer token is never part of it.

**Signature:** `def _raise_for_kv_error(response, action)`

**See also:** `test_a_200_response_is_accepted`, `test_a_non_200_with_an_errors_list_names_the_first_error`, `test_a_non_200_with_an_empty_errors_array_still_raises_with_the_body_text`, `test_a_non_json_error_page_still_raises_with_its_text`; `github_message:kv_get_keys`, `github_message:kv_get_value`.

#### `kv_get_keys` [@ANCHOR: github_message:kv_get_keys]

Lists the Workers KV keys whose names start with a prefix. `main` calls it with `update_<version>_`, so the result holds one key per binary that `github_release.py` uploaded for that release; each key's value is that binary's checksum.

| Argument | Type | Meaning |
|---|---|---|
| `prefix` | str | key-name prefix, sent as the `prefix` query parameter |
| `account` | str | Cloudflare account id used in the URL path |
| `namespace` | str | Workers KV namespace id used in the URL path |
| `api_token` | str | Cloudflare API token, sent as `Authorization: Bearer` |

**Returns:** The `result` list of the answer: dictionaries such as `{"name": "update_2026.10.7_cloudflared-linux-amd64"}`; `main` reads only `name`.

**Errors:** Anything `_raise_for_kv_error` raises (non-200 answer); a network failure from `requests` propagates unchanged.

**Special concerns:** Blocks on one HTTPS GET to `api.cloudflare.com` with no timeout argument and no retry. Reads only; changes nothing on either side. The API token is a secret and goes only into the request header. A single page is read, so a namespace with more matching keys than the API returns per page would be cut short (not checked here: unknown, to-do if the key count ever approaches the API's page limit).

**Signature:** `def kv_get_keys(prefix, account, namespace, api_token)`

**See also:** `test_keys_are_listed_with_the_prefix_and_the_bearer_token`, `test_a_failed_listing_raises_instead_of_returning_nothing`.

#### `kv_get_value` [@ANCHOR: github_message:kv_get_value]

Reads one Workers KV value, which for this script is the sha256 checksum text of one uploaded binary. `main` calls it once for every key `kv_get_keys` returned. The key is percent-encoded with no safe characters, so a key containing `/` or other reserved characters cannot change the URL path.

| Argument | Type | Meaning |
|---|---|---|
| `key` | str | full key name, for example `update_<version>_<binary>` |
| `account` | str | Cloudflare account id |
| `namespace` | str | Workers KV namespace id |
| `api_token` | str | Cloudflare API token (Bearer) |

**Returns:** The response body text, unparsed and untrimmed.

**Errors:** `_raise_for_kv_error` raises for any non-200 answer, so error text is never returned as a value; network errors propagate.

**Special concerns:** Blocks on one HTTPS GET without timeout or retry; read-only. Returns exactly what was stored, so the value is whatever `github_release.send_hash` put there; it is not validated as a hex digest here.

**Signature:** `def kv_get_value(key, account, namespace, api_token)`

**See also:** `test_a_value_is_fetched_by_its_percent_encoded_key`, `test_a_failed_value_fetch_never_returns_the_error_text_as_a_checksum`.

#### `update_or_add_message` [@ANCHOR: github_message:update_or_add_message]

Sets the `name: sha` line for one binary inside the release-notes text. It replaces that binary's own line, else appends the line inside the existing code block, else starts a new `### SHA256 Checksums:` heading with a fresh code block. `main` calls it once per binary, starting from an empty string, so the result is the whole notes body.

| Argument | Type | Meaning |
|---|---|---|
| `msg` | str | the notes built so far, possibly empty |
| `name` | str | binary name such as `cloudflared-linux-amd64` |
| `sha` | str | checksum text as returned by `kv_get_value` |

**Returns:** The new notes text. The existing line is replaced (first match only) by `name: sha` plus a newline.

**Errors:** None raised; a `name` containing regular-expression characters is escaped.

**Special concerns:** Pure string function. The existing-line match is anchored at the start of a line (`^name: `), so `cloudflared-linux-amd64` never matches the `cloudflared-linux-amd64-fips` line; the result does not depend on the order in which KV lists keys. The closing fence is found with `rfind` of three backticks, so a notes body with a later unrelated code block would receive the line in that block (unknown whether any release notes have one; the script starts from empty notes, so not in the normal path).

**Signature:** `def update_or_add_message(msg, name, sha)`

**See also:** `test_the_first_checksum_creates_the_heading_and_the_code_block`, `test_a_second_checksum_is_added_inside_the_existing_block`, `test_an_existing_checksum_is_replaced_not_duplicated`, `test_a_name_that_is_a_prefix_of_another_does_not_clobber_the_longer_one`.

#### `get_release` [@ANCHOR: github_message:get_release]

Fetches the GitHub release whose tag equals the version, and answers `None` when GitHub says no such release exists. `main` uses it to decide whether there is anything to update; it creates nothing (unlike `github_release.get_or_create_release`).

| Argument | Type | Meaning |
|---|---|---|
| `repo` | PyGithub `Repository` | the repository object from `Github(api_key).get_repo(...)` |
| `version` | str | release tag, for example `2026.10.7` |

**Returns:** The PyGithub release object, or `None` (implicitly) after `UnknownObjectException`.

**Errors:** Only `UnknownObjectException` is caught; any other GitHub error (rate limit, authentication, network) propagates to `main`, which logs it and exits with status 1.

**Special concerns:** One GitHub API read; logs `Release <v> found` or `not found` at INFO. No local state or files.

**Signature:** `def get_release(repo, version)`

**See also:** `test_an_existing_release_is_returned`, `test_a_missing_release_gives_none`.

#### `parse_args` [@ANCHOR: github_message:parse_args]

Reads and validates the command line for `github_message.py`. Each value may also come from an environment variable, which is how the Makefile target passes secrets without putting them in the process list. A missing value is reported by name through the logger and the script stops.

| Argument | Type | Meaning |
|---|---|---|
| `--api-key` | str | GitHub token; default env `API_KEY` |
| `--kv-namespace-id` | str | Workers KV namespace; env `KV_NAMESPACE` |
| `--kv-account-id` | str | Cloudflare account; env `KV_ACCOUNT` |
| `--kv-api-token` | str | Cloudflare API token; env `KV_API_TOKEN` |
| `--release-version` | str | release tag; env `VERSION` |
| `--dry-run` | flag | do not write the release notes |

**Returns:** The `argparse.Namespace` once all five required values are non-empty.

**Errors:** If any required value is missing it logs `Missing ...` for each, prints usage, and calls `exit(1)` (`SystemExit` status 1). `argparse` itself exits with status 2 on unknown flags.

**Special concerns:** Reads `sys.argv` and `os.environ` only. The tokens are never logged by this function. Command-line values are visible to other users in `ps`; prefer the environment variables for the two tokens.

**Signature:** `def parse_args()`

**See also:** `test_every_value_can_come_from_the_command_line`, `test_every_value_can_come_from_the_environment`, `test_a_missing_required_value_exits_with_status_1`.

#### `main` [@ANCHOR: github_message:main]

Entry point: builds the checksum notes for one release from Workers KV and writes them to the GitHub release. It lists the keys with the prefix `update_<version>_`, reads each value, adds a line per binary, and then (unless `--dry-run`) calls `release.update_release(version, msg)`, replacing the release body.

**Returns:** Nothing. On a dry run it logs the would-be notes and returns without touching the release.

**Errors:** Any exception is logged with its traceback and the script exits with status 1. A missing release (with no `--dry-run`) raises `Release <v> not found; cannot update message` instead of an `AttributeError` on `None`.

**Special concerns:** Several sequential HTTPS calls (one GitHub lookup, one KV listing, one KV read per binary) with no timeout and no concurrency. The only write is the GitHub release body, which is overwritten, not merged: notes written by hand are lost. Needs a GitHub token with write access and a KV token; both are secrets. Not used on hams1 or in hams.com production.

**Signature:** `def main()`

**See also:** `test_the_release_notes_get_one_checksum_line_per_binary`, `test_a_dry_run_changes_nothing`, `test_a_missing_release_fails_with_a_clear_exit_not_an_attribute_error`; the other `github_message:*` anchors.

### `github_release.py`: upload binaries to the release

Uploads the built binaries as GitHub release assets and records each one's sha256 in Workers KV. Run as a script from the `github-release` and `github-release-dryrun` Makefile targets; tests: `test_github_release.py`.

#### `get_sha256` [@ANCHOR: github_release:get_sha256]

Computes the hex sha256 digest of a file. `upload_asset` uses it to produce the checksum that `send_hash` stores in Workers KV for the updater to verify.

| Argument | Type | Meaning |
|---|---|---|
| `filename` | str | path of the file to hash |

**Returns:** The 64-character lowercase hex digest as `str`.

**Errors:** `OSError` (missing file, permission) propagates.

**Special concerns:** Reads the whole file sequentially in 4 KiB blocks, so memory stays constant for large binaries. No state, no network.

**Signature:** `def get_sha256(filename)`

**See also:** `test_the_digest_matches_hashlib_for_a_file_larger_than_one_block`, `test_the_digest_of_an_empty_file_is_the_well_known_empty_digest`.

#### `send_hash` [@ANCHOR: github_release:send_hash]

Stores one binary's checksum in Workers KV under the key `update_<version>_<name>`. That key shape is what `github_message.py` later lists with the prefix `update_<version>_`, and what the cloudflared updater reads.

| Argument | Type | Meaning |
|---|---|---|
| `pkg_hash` | str | hex digest from `get_sha256`; sent as the request body |
| `name` | str | asset file name, last part of the key |
| `version` | str | release version, middle part of the key |
| `account` | str | Cloudflare account id |
| `namespace` | str | Workers KV namespace id |
| `api_token` | str | Cloudflare API token (Bearer) |

**Returns:** Nothing.

**Errors:** Any status other than 200 raises `Exception("failed to upload checksum: <detail>")`; the detail is the first entry of the body's `errors` array, or the body text when the array is empty, missing or the body is not JSON.

**Special concerns:** One HTTPS PUT without timeout or retry; it overwrites an existing value, so it is safe to repeat. The key is percent-encoded. The token is a secret and only goes in the header.

**Signature:** `def send_hash(pkg_hash, name, version, account, namespace, api_token)`

**See also:** `test_the_checksum_is_put_under_the_update_version_name_key`, `test_a_rejected_upload_raises_even_when_the_errors_array_is_empty`, `test_a_rejected_upload_names_the_first_error_from_the_body`; `github_message:kv_get_keys`.

#### `assert_tag_exists` [@ANCHOR: github_release:assert_tag_exists]

Checks that the repository has a git tag exactly equal to the version before a release is created for it, so a release is never made for a mistyped version. `get_or_create_release` calls it only when no release exists yet.

| Argument | Type | Meaning |
|---|---|---|
| `repo` | PyGithub `Repository` | repository whose tags are listed |
| `version` | str | tag name that must exist |

**Returns:** Nothing when found.

**Errors:** Raises `Exception("Tag <version> not found")` when no tag matches, including when the repository has no tags.

**Special concerns:** Iterates over every tag the API returns (paged by PyGithub, one request per page), comparing names for equality, so the result does not depend on list order and a tag that is not first is found. Read-only.

**Signature:** `def assert_tag_exists(repo, version)`

**See also:** `test_a_tag_that_is_not_first_in_the_list_is_found`, `test_a_missing_tag_raises_naming_the_tag`, `test_a_repository_with_no_tags_raises_the_not_found_error`.

#### `get_or_create_release` [@ANCHOR: github_release:get_or_create_release]

Returns the GitHub release for the version tag, creating it when it does not exist. Several build jobs upload different binaries to the same release at about the same time, so the creation race is expected and handled: when GitHub answers 422 `already_exists`, the release the other job just made is fetched instead.

| Argument | Type | Meaning |
|---|---|---|
| `repo` | PyGithub `Repository` | target repository |
| `version` | str | tag and release name |
| `dry_run` | bool, default False | when true nothing is created |
| `is_draft` | bool, default False | create the release as a draft |

**Returns:** The release object; `None` on a dry run when the release does not exist yet.

**Errors:** `Exception` from `assert_tag_exists` when the tag is missing; any other `GithubException` (not the 422 conflict) is re-raised.

**Special concerns:** GitHub API calls with no retry; the only write is `create_git_release(version, version, "", is_draft)` (name and tag both the version, empty body). It does not set release notes or attach assets. Needs a token with write access.

**Signature:** `def get_or_create_release(repo, version, dry_run=False, is_draft=False)`

**See also:** `test_an_existing_release_is_returned_without_creating_one`, `test_a_new_release_is_created_for_an_existing_tag`, `test_no_release_is_created_for_a_tag_that_does_not_exist`, `test_a_dry_run_creates_nothing`, `test_losing_a_creation_race_fetches_the_release_the_other_build_made`, `test_any_other_creation_error_propagates`.

#### `parse_args` [@ANCHOR: github_release:parse_args]

Reads and validates the command line for `github_release.py`, taking each value from a flag or else from an environment variable. The asset name is required only when the path is a file; for a directory the file names are used.

| Argument | Type | Meaning |
|---|---|---|
| `--api-key` | str | GitHub token; env `API_KEY` |
| `--release-version` | str | release tag; env `VERSION` |
| `--path` | str | asset file or directory; env `ASSET_PATH` |
| `--name` | str | asset name; env `ASSET_NAME` |
| `--namespace-id` | str | Workers KV namespace; env `KV_NAMESPACE` |
| `--kv-account-id` | str | Cloudflare account; env `KV_ACCOUNT` |
| `--kv-api-token` | str | Cloudflare API token; env `KV_API_TOKEN` |
| `--dry-run` | flag | only check binary versions |
| `--draft` | flag | create the release as a draft |

**Returns:** The `argparse.Namespace` when everything required is present.

**Errors:** Each missing required value is logged (`Missing ...`), usage is printed and `exit(1)` is called.

**Special concerns:** Reads `sys.argv`, `os.environ` and tests `os.path.isdir(--path)`; no secrets are logged. Prefer the environment for the two tokens, since flags show in `ps`.

**Signature:** `def parse_args()`

**See also:** `test_all_values_given_on_the_command_line_are_returned`, `test_values_come_from_the_environment_when_no_flag_is_given`, `test_a_missing_asset_name_is_fine_when_the_path_is_a_directory`, `test_a_missing_required_value_exits_with_status_1`.

#### `upload_asset` [@ANCHOR: github_release:upload_asset]

Uploads one file to the release as an asset, unless an asset of that name is already there, and always sends the file's checksum to Workers KV. The checksum is deliberately sent even for an already-uploaded asset, so a retry after a crash between the upload and the checksum still records it.

| Argument | Type | Meaning |
|---|---|---|
| `release` | PyGithub release | target release |
| `filepath` | str | local file |
| `filename` | str | asset name on the release and KV key suffix |
| `release_version` | str | version for the KV key |
| `kv_account_id, namespace_id, kv_api_token` | str | Workers KV coordinates and token |

**Returns:** Nothing.

**Errors:** GitHub or KV errors propagate (`send_hash` raises on non-200); a corrupt `.tgz` raises from `tarfile`.

**Special concerns:** For a name ending in `tgz`, it deletes and recreates the directory `cfd` under the current directory, extracts the archive there, and hashes `cfd/cloudflared` (the binary inside), not the archive. It therefore needs write access to the working directory and must not run twice in one directory concurrently. The extraction uses `extractall` on a trusted build artifact (not hardened against hostile archives).

**Signature:** `def upload_asset(release, filepath, filename, release_version, kv_account_id, namespace_id, kv_api_token)`

**See also:** `test_a_new_asset_is_uploaded_and_its_checksum_sent`, `test_an_already_uploaded_asset_is_not_uploaded_again_but_its_checksum_is_still_sent`, `test_a_tgz_asset_has_the_checksum_of_the_binary_inside_it`; `github_release:send_hash`.

#### `move_asset` [@ANCHOR: github_release:move_asset]

Copies an uploaded file into `./artifacts/` under its asset name, creating the directory if necessary. The later packaging step (`release_pkgs.py`) looks for the `.deb` and `.rpm` files there. A file that already is that very file (the macOS release job writes into `artifacts` itself) is left alone.

| Argument | Type | Meaning |
|---|---|---|
| `filepath` | str | source file |
| `filename` | str | name to use inside `artifacts` |

**Returns:** Nothing.

**Errors:** `OSError` from the directory creation or copy propagates; only `shutil.SameFileError` is swallowed.

**Special concerns:** Writes under the current working directory (`artifacts/`); not safe for concurrent runs in one directory for the same name. Copies, never moves, despite the name.

**Signature:** `def move_asset(filepath, filename)`

**See also:** `test_the_file_is_copied_into_a_new_artifacts_directory`, `test_a_file_already_in_the_artifacts_directory_is_left_alone`.

#### `get_binary_version` [@ANCHOR: github_release:get_binary_version]

Reads the version a built Go binary has embedded. It runs `go version -m <binary>`, finds the `main.Version=` marker that the build's `-ldflags` set, and returns the text up to the next backslash.

| Argument | Type | Meaning |
|---|---|---|
| `binary_path` | str | path of the executable to inspect |

**Returns:** The version string, for example `2024.8.3-6-gec072691`.

**Errors:** Raises `Exception` naming the marker and showing the output when `main.Version=` is not found (the `go` tool failed, its output format changed, or the binary has no version information). Failing to start `go` raises `OSError`.

**Special concerns:** Spawns a subprocess (`go`, which must be on `PATH`) and waits for it; read-only. stderr is captured and discarded. The parse is positional (up to the first backslash after the marker), so a changed ldflags quoting would break it.

**Signature:** `def get_binary_version(binary_path)`

**See also:** `test_the_version_is_cut_out_between_the_marker_and_the_backslash`, `test_output_without_the_marker_raises_instead_of_returning_garbage`.

#### `assert_asset_version` [@ANCHOR: github_release:assert_asset_version]

Guards against uploading a binary built from the wrong version. It reads the embedded version with `get_binary_version` and exits the process when it differs from the release version. Only `.exe`, extensionless files and `.tgz` archives are checked; other extensions (`.deb`, `.rpm`, `.msi` and so on) return without a check.

| Argument | Type | Meaning |
|---|---|---|
| `binary_path` | str | file to check |
| `release_version` | str | version that the binary must report |

**Returns:** Nothing when equal or when the extension is not checked.

**Errors:** Logs `Version mismatch ...` and calls `exit(1)` on a mismatch; errors from `get_binary_version` or `tarfile` propagate.

**Special concerns:** Deletes the directory `tmp` in the current directory first, then for `.tgz` extracts into `tmp` and inspects `tmp/cloudflared`; runs `go` as a subprocess. Not safe to run twice concurrently in the same directory.

**Signature:** `def assert_asset_version(binary_path, release_version)`

**See also:** `test_a_matching_extensionless_binary_passes`, `test_a_mismatching_version_exits_with_status_1`, `test_a_file_with_another_extension_is_not_checked`, `test_a_tgz_is_unpacked_and_the_binary_inside_is_checked`; `github_release:get_binary_version`.

#### `main` [@ANCHOR: github_release:main]

Entry point of `github_release.py`. With `--dry-run` it only checks the version of the file or of every file in the directory and uploads nothing. Without it, `--path` must be a directory: every file's version is checked first, then the release is found or created, then each file is uploaded (with its checksum) and copied to `artifacts`.

**Returns:** Nothing.

**Errors:** Everything is caught, logged with a traceback, and turned into exit status 1. A real run with a path that is a single file raises `the argument path must be a directory`; a dry run with a path that is neither file nor directory logs `dryrun failed` and returns normally.

**Special concerns:** Network calls to GitHub and Workers KV, one file at a time and sequentially, with no timeouts. Checking all versions before creating the release means a wrong binary stops the run before anything is published. Writes `artifacts/`, `tmp/` and `cfd/` in the working directory. Needs GitHub and KV tokens (secrets).

**Signature:** `def main()`

**See also:** `test_a_single_file_dry_run_checks_that_files_version_without_a_name_error`, `test_a_directory_dry_run_checks_every_file_and_uploads_nothing`, `test_a_real_run_checks_versions_then_makes_the_release_then_uploads_and_moves_each_file`, `test_a_real_run_with_a_single_file_path_exits_with_status_1`.

### `release_pkgs.py`: signed apt and yum repositories on R2

Builds the `.deb` (reprepro) and `.rpm` (createrepo_c) repositories, signs them, and uploads them to Cloudflare R2 (Cloudflare's S3-compatible object storage). Run as a script from the `r2-linux-release` Makefile targets; tests: `test_release_pkgs.py`. All paths are relative to the current directory (`./conf`, `./artifacts`, `./rpm`, `dists`, `pool`).

#### `PkgUploader.__init__` [@ANCHOR: release_pkgs:PkgUploader::__init__]

Constructor of the R2 uploader: it stores the four values that identify the account, bucket and credentials. It makes no connection; the client is built later in `upload_pkg_to_r2`.

| Argument | Type | Meaning |
|---|---|---|
| `account_id` | str | Cloudflare account id, used to form the R2 endpoint host |
| `bucket_name` | str | destination R2 bucket |
| `client_id` | str | R2 access key id (secret) |
| `client_secret` | str | R2 secret access key (secret) |

**Returns:** A new `PkgUploader` with the four attributes `account_id`, `bucket_name`, `client_id`, `client_secret`.

**Errors:** None.

**Special concerns:** Holds secrets in plain attributes for the life of the process; no thread use, no I/O.

**Signature:** `def __init__(self, account_id, bucket_name, client_id, client_secret)`

**See also:** `test_the_constructor_keeps_the_four_r2_settings`.

#### `PkgUploader.upload_pkg_to_r2` [@ANCHOR: release_pkgs:PkgUploader::upload_pkg_to_r2]

Uploads one local file to the bucket at a given key, through the account's S3-compatible endpoint (`https://<account>.r2.cloudflarestorage.com`, region `auto`, path-style addressing). `upload_from_directories` and `create_rpm_packaging` call it once per file.

| Argument | Type | Meaning |
|---|---|---|
| `filename` | str | local file path to read |
| `upload_file_path` | str | object key inside the bucket |

**Returns:** Nothing.

**Errors:** A `botocore` `ClientError` (rejected credentials, missing bucket, and so on) is re-raised to the caller; local file errors propagate too.

**Special concerns:** Creates a new `boto3` client on every call and blocks for the whole upload; it overwrites an existing object. Prints the upload line. Network side effect on production package storage, so a real run changes what apt and yum users download.

**Signature:** `def upload_pkg_to_r2(self, filename, upload_file_path)`

**See also:** `test_a_file_is_uploaded_to_the_accounts_r2_endpoint_with_the_clients_keys`, `test_an_r2_client_error_propagates_to_the_caller`.

#### `PkgCreator.create_distribution_conf` [@ANCHOR: release_pkgs:PkgCreator::create_distribution_conf]

Writes reprepro's `distributions` file, which tells reprepro (the Debian repository tool) which releases to serve and which key signs them. One stanza is written per release with `Origin`, `Label`, `Codename`, `Architectures`, `Components`, `Description` (suffixed with the release) and `SignWith`, separated by blank lines.

| Argument | Type | Meaning |
|---|---|---|
| `file_path` | str | file to write, normally `./conf/distributions` |
| `origin, label` | str | repository names, both the binary name in practice |
| `releases` | list of str | Debian or Ubuntu codenames, one stanza each |
| `archs` | list of str | architectures, joined with spaces |
| `components` | str | component name, `main` |
| `description` | str | free text |
| `gpg_key_id` | str | text for `SignWith`: one fingerprint, or two separated by a space during a key rollover |

**Returns:** The (closed) file object, as the code returns it.

**Errors:** `OSError` if the file cannot be opened; the directory must exist (`create_deb_packaging` creates `./conf`).

**Special concerns:** Truncates and rewrites the file. No network, no signing happens here.

**Signature:** `def create_distribution_conf(self, file_path, origin, label, releases, archs, components, description, gpg_key_id)`

**See also:** `test_one_stanza_is_written_per_release_with_the_signing_key`.

#### `PkgCreator.create_deb_pkgs` [@ANCHOR: release_pkgs:PkgCreator::create_deb_pkgs]

Adds one `.deb` file to one release of the apt repository by running `reprepro includedeb`. reprepro creates and fills the `db`, `pool` and `dists` directories, and signs the `InRelease` file as the `distributions` file says. Called once per release and architecture by `create_deb_packaging`.

| Argument | Type | Meaning |
|---|---|---|
| `release` | str | codename such as `bookworm` |
| `deb_file` | str | path of the `.deb` to include |

**Returns:** Nothing.

**Errors:** A non-zero exit raises `RuntimeError("reprepro includedeb <release> <file> failed (exit N): <stderr>")`; stdout and stderr are also printed.

**Special concerns:** Runs `reprepro` as a subprocess in the current directory and waits; modifies `./db`, `./pool` and `./dists`. The signing key must already be in the gpg keyring (`import_gpg_keys`). Not safe to run concurrently in one directory.

**Signature:** `def create_deb_pkgs(self, release, deb_file)`

**See also:** `test_reprepro_is_asked_to_include_the_deb_in_the_release`, `test_a_reprepro_failure_raises_with_the_exit_code_and_stderr`.

#### `PkgCreator.create_rpm_pkgs` [@ANCHOR: release_pkgs:PkgCreator::create_rpm_pkgs]

Builds the yum repository. It sorts and signs the `.rpm` files into `./rpm/<arch>/`, runs `createrepo_c ./rpm` to produce the metadata, and then signs `repomd.xml` with a detached signature, in that order.

| Argument | Type | Meaning |
|---|---|---|
| `artifacts_path` | str | directory searched for `.rpm` files |
| `gpg_key_name` | str | key name passed to `rpm` for signing the packages |

**Returns:** Nothing.

**Errors:** A non-zero `createrepo_c` exit raises `RuntimeError` with the exit code and stderr, before any metadata is signed; errors from the signing helpers propagate.

**Special concerns:** Subprocesses (`createrepo_c`, `rpm`, `gpg`) in the current directory; creates `./rpm`. Repository metadata is signed only after it is complete, so an unsigned repository is never published by this step alone.

**Signature:** `def create_rpm_pkgs(self, artifacts_path, gpg_key_name)`

**See also:** `test_the_rpms_are_set_up_the_repo_is_built_and_the_metadata_is_signed_in_that_order`, `test_a_createrepo_failure_raises_and_nothing_is_signed`; `release_pkgs:PkgCreator::_setup_rpm_pkg_directories`, `release_pkgs:PkgCreator::_sign_repomd`.

#### `PkgCreator.create_repo_file` [@ANCHOR: release_pkgs:PkgCreator::create_repo_file]

Writes the `<binary>.repo` file that yum users download to add the repository. It defines a section `[<binary>-stable]` with `baseurl=<baseurl>/rpm`, `enabled=1`, `type=rpm`, `gpgcheck=1` and the URL of the public key.

| Argument | Type | Meaning |
|---|---|---|
| `file_path` | str | directory in which to write the file |
| `binary_name` | str | name used for the file and the section |
| `baseurl` | str | public URL of the package host, without `/rpm` |
| `gpgkey_url` | str | URL of the public signing key |

**Returns:** The path of the written file.

**Errors:** `OSError` when the directory is missing or not writable.

**Special concerns:** Overwrites an existing file; no network. `gpgcheck=1` makes yum refuse unsigned packages, so the key URL must really serve the key that signed them.

**Signature:** `def create_repo_file(self, file_path, binary_name, baseurl, gpgkey_url)`

**See also:** `test_the_dot_repo_file_points_yum_at_the_rpm_directory_and_requires_a_signature`.

#### `PkgCreator._sign_rpms` [@ANCHOR: release_pkgs:PkgCreator::_sign_rpms]

Signs one `.rpm` file in place by running `rpm --define "_gpg_name <key>" --addsign <file>`. Used for each copied package by `_setup_rpm_pkg_directories`.

| Argument | Type | Meaning |
|---|---|---|
| `file_path` | str | the `.rpm` to sign |
| `gpg_key_name` | str | key name (user id) that rpm should use |

**Returns:** Nothing.

**Errors:** A non-zero exit raises `RuntimeError("rpm --addsign <file> failed (exit N): <stderr>")`.

**Special concerns:** Subprocess `rpm`; it modifies the file and needs the secret key in the gpg keyring without a passphrase prompt.

**Signature:** `def _sign_rpms(self, file_path, gpg_key_name)`

**See also:** `test_rpm_addsign_is_run_with_the_named_key`, `test_a_signing_failure_raises_with_the_real_error`.

#### `PkgCreator._sign_repomd` [@ANCHOR: release_pkgs:PkgCreator::_sign_repomd]

Writes an armored detached gpg signature next to the repository metadata (`./rpm/repodata/repomd.xml.asc`), by running `gpg --batch --yes --detach-sign --armor ./rpm/repodata/repomd.xml`. yum uses it to check the metadata when `repo_gpgcheck` is on.

**Returns:** Nothing.

**Errors:** A non-zero exit raises `RuntimeError("gpg --detach-sign ./rpm/repodata/repomd.xml failed (exit N): <stderr>")`.

**Special concerns:** Subprocess `gpg`; it uses the default secret key of the keyring (no key is named), so with two imported keys the choice is gpg's default (unknown which; to-do if the rollover needs both). Overwrites an existing signature file.

**Signature:** `def _sign_repomd(self)`

**See also:** `test_the_repository_metadata_gets_an_armored_detached_signature`, `test_a_metadata_signing_failure_raises_with_the_real_error`.

#### `PkgCreator._setup_rpm_pkg_directories` [@ANCHOR: release_pkgs:PkgCreator::_setup_rpm_pkg_directories]

Arranges the `.rpm` files into the layout createrepo expects: for each architecture it walks the artifacts directory, copies every file whose name ends in `<arch>.rpm` to `./rpm/<arch>/` and signs the copy with `_sign_rpms`. It assumes names of the form `<prefix>-<arch>.rpm`.

| Argument | Type | Meaning |
|---|---|---|
| `artifacts_path` | str | directory tree to search |
| `gpg_key_name` | str | key name for signing |
| `archs` | list of str, default `["aarch64", "x86_64", "386"]` | architectures to collect |

**Returns:** Nothing.

**Errors:** `OSError` from the copy; `RuntimeError` from `_sign_rpms`.

**Special concerns:** Creates `./rpm/<arch>` directories and copies files; the originals stay unsigned. The match is a plain suffix test on the file name.

**Signature:** `def _setup_rpm_pkg_directories(self, artifacts_path, gpg_key_name, archs=["aarch64", "x86_64", "386"])`

**See also:** `test_each_rpm_is_copied_under_its_architecture_and_signed`.

#### `PkgCreator.import_gpg_keys` [@ANCHOR: release_pkgs:PkgCreator::import_gpg_keys]

Imports a base64-encoded private key and public key into the gpg keyring, so reprepro, rpm and gpg can sign packages, and returns which key it is. The returned fingerprint goes to reprepro's `SignWith`; the user id is used as the rpm key name.

| Argument | Type | Meaning |
|---|---|---|
| `private_key` | str | base64 text of the armored private key (secret) |
| `public_key` | str | base64 text of the public key |

**Returns:** A tuple `(fingerprint, first user id)` of the imported secret key.

**Errors:** `Exception("Failed to import private key")` when gpg reports no fingerprint; `Exception("Could not find imported key with fingerprint ...")` when the key is not in the secret-key list after import.

**Special concerns:** Writes to the gpg home (default `~/.gnupg`) of the running user through `python-gnupg`; the private key sits there until removed, so run it only on a build host. The key material is never printed by this function.

**Signature:** `def import_gpg_keys(self, private_key, public_key)`

**See also:** `test_the_decoded_keys_are_imported_and_the_fingerprint_and_uid_are_returned`, `test_a_private_key_that_imports_no_fingerprint_is_rejected`, `test_an_imported_key_missing_from_the_secret_key_list_is_an_error`.

#### `PkgCreator.import_multiple_gpg_keys` [@ANCHOR: release_pkgs:PkgCreator::import_multiple_gpg_keys]

Imports the primary keypair and, during a key rollover, a secondary keypair, using `import_gpg_keys` for each. A pair is imported only when both its private and public halves are given; an incomplete pair is silently skipped.

| Argument | Type | Meaning |
|---|---|---|
| `primary_private_key, primary_public_key` | str | base64 keys of the primary pair |
| `secondary_private_key, secondary_public_key` | str or None, default None | base64 keys of the rollover pair |

**Returns:** A list of `(fingerprint, uid)` tuples, primary first; empty when no complete pair was given (the `__main__` block then exits with `No GPG keys were provided for signing`).

**Errors:** Errors from `import_gpg_keys` propagate.

**Special concerns:** Modifies the gpg keyring as `import_gpg_keys` does. A half-given pair is ignored without a warning.

**Signature:** `def import_multiple_gpg_keys(self, primary_private_key, primary_public_key, secondary_private_key=None, secondary_public_key=None)`

**See also:** `test_the_primary_key_comes_first_and_the_secondary_second`, `test_a_half_given_pair_is_ignored_and_no_keys_gives_an_empty_list`.

#### `PkgCreator.import_rpm_key` [@ANCHOR: release_pkgs:PkgCreator::import_rpm_key]

Runs the equivalent of `rpm --import <key file>` for the public key, which rpm needs in order to sign packages with it. The base64 text is decoded into a file named `pb.key` in the current directory first.

| Argument | Type | Meaning |
|---|---|---|
| `public_key` | str | base64 text of the public key |

**Returns:** Nothing.

**Errors:** A non-zero exit raises `RuntimeError("rpm --import pb.key failed (exit N): <stderr>")`.

**Special concerns:** Writes `pb.key` (left behind afterwards) and changes the rpm database of the build host. A public key, not a secret.

**Signature:** `def import_rpm_key(self, public_key)`

**See also:** `test_the_decoded_public_key_is_written_and_given_to_rpm_import`, `test_an_rpm_import_failure_raises_with_the_real_error`.

#### `upload_from_directories` [@ANCHOR: release_pkgs:upload_from_directories]

Uploads every file under a directory to R2, at the key `[<release>/]<binary>/<path of the file>`. `create_deb_packaging` and `create_rpm_packaging` use it for `dists`, `pool` and `rpm`, once as the latest repository and optionally again under the release version as a permanent backup.

| Argument | Type | Meaning |
|---|---|---|
| `pkg_uploader` | `PkgUploader` | uploader used for each file |
| `directory` | str | root to walk; the walked path is part of the key |
| `release` | str or None | version prefix; None means no prefix |
| `binary` | str | binary name, first path part below the prefix |

**Returns:** Nothing.

**Errors:** An upload error propagates and stops the walk, so a half-published repository fails with a non-zero exit instead of looking successful. An older version swallowed the error.

**Special concerns:** Sequential uploads, one network request per file, in `os.walk` order (so `InRelease` may be uploaded before all packages; a failure mid-way leaves a partial remote tree). Overwrites existing objects.

**Signature:** `def upload_from_directories(pkg_uploader, directory, release, binary)`

**See also:** `test_every_file_is_uploaded_under_the_binary_name_and_no_release_prefix`, `test_a_release_argument_adds_a_versioned_prefix`, `test_a_failed_upload_stops_the_run_instead_of_being_swallowed`.

#### `create_deb_packaging` [@ANCHOR: release_pkgs:create_deb_packaging]

Builds and publishes the apt repository. It creates `./conf`, writes the reprepro configuration (naming both keys in `SignWith` during a rollover), includes `./artifacts/<binary>-linux-<arch>.deb` for every release and architecture, then uploads `dists` and `pool` as the latest repository and, when a release version is given, again under that version.

| Argument | Type | Meaning |
|---|---|---|
| `pkg_creator` | `PkgCreator` | builds the configuration and packages |
| `pkg_uploader` | `PkgUploader` | uploads to R2 |
| `releases` | list of str | distribution codenames |
| `primary_gpg_key_id` | str | fingerprint of the signing key |
| `secondary_gpg_key_id` | str or None | second fingerprint for rollover |
| `binary_name` | str | name used in file names and R2 keys |
| `archs` | list of str | architectures whose `.deb` files must already be in `./artifacts` |
| `package_component` | str | component, `main` from the script |
| `release_version` | str or None | version prefix for the backup upload |

**Returns:** Nothing.

**Errors:** `RuntimeError` when reprepro fails (for example a missing `.deb`); upload errors propagate.

**Special concerns:** Writes `./conf/distributions` and reprepro's directories, runs one subprocess per release and architecture, then uploads over the network, which changes the live repository.

**Signature:** `def create_deb_packaging(pkg_creator, pkg_uploader, releases, primary_gpg_key_id, secondary_gpg_key_id, binary_name, archs, package_component, release_version)`

**See also:** `test_the_conf_lists_both_signing_keys_in_rollover_and_each_release_arch_deb_is_included`, `test_one_key_signs_alone_and_a_release_version_also_makes_a_versioned_copy`.

#### `create_rpm_packaging` [@ANCHOR: release_pkgs:create_rpm_packaging]

Builds and publishes the yum repository: it calls `create_rpm_pkgs`, writes the `.repo` file into the artifacts directory, uploads it as `<binary>.repo`, uploads the `rpm` tree as the latest, and with `upload_repo_file` also uploads the tree under the release version.

| Argument | Type | Meaning |
|---|---|---|
| `pkg_creator, pkg_uploader` | objects | as for `create_deb_packaging` |
| `artifacts_path` | str | directory holding the `.rpm` files and receiving the `.repo` file |
| `release_version` | str or None | prefix for the versioned upload |
| `binary_name` | str | name for the repo file and R2 keys |
| `gpg_key_name` | str | key name for rpm signing |
| `base_url, gpg_key_url` | str | put into the `.repo` file |
| `upload_repo_file` | bool, default False | also upload the versioned copy |

**Returns:** Nothing.

**Errors:** Errors from `create_rpm_pkgs`, `create_repo_file` and the uploads propagate.

**Special concerns:** Runs subprocesses and network uploads that change the live repository. The `.repo` file is uploaded at the bucket root, not under the binary name. The `__main__` block passes the secondary key's user id as `gpg_key_name`, which is `None` when no secondary key was given (unknown whether that signs correctly; to-do for the owner of the release pipeline).

**Signature:** `def create_rpm_packaging(pkg_creator, pkg_uploader, artifacts_path, release_version, binary_name, gpg_key_name, base_url, gpg_key_url, upload_repo_file=False)`

**See also:** `test_the_repo_file_is_uploaded_and_the_rpm_tree_is_uploaded_as_latest`, `test_upload_repo_file_adds_the_versioned_copy`.

#### `parse_args` [@ANCHOR: release_pkgs:parse_args]

Reads and validates the command line for `release_pkgs.py`, each value from a flag or an environment variable. Only the R2 settings and the binary name are checked; a missing GPG key is detected later by the `__main__` block.

| Argument | Type | Meaning |
|---|---|---|
| `--bucket, --id, --secret, --account` | str | R2 bucket, client id, client secret, account; env `R2_BUCKET`, `R2_CLIENT_ID`, `R2_CLIENT_SECRET`, `R2_ACCOUNT_ID` |
| `--binary` | str | binary name; env `BINARY_NAME` |
| `--release-tag` | str | version for the versioned upload; env `RELEASE_VERSION` |
| `--gpg-private-key(-2), --gpg-public-key(-2)` | str | base64 keys, primary and optional secondary; env `LINUX_SIGNING_PRIVATE_KEY(_2)`, `LINUX_SIGNING_PUBLIC_KEY(_2)` |
| `--gpg-public-key-url, --pkg-upload-url` | str | URLs for the `.repo` file; env `GPG_PUBLIC_KEY_URL`, `PKG_URL` |
| `--deb-based-releases, --archs` | list | defaults `any bookworm noble jammy focal bionic xenial` and `amd64 386 arm64 arm armhf` |
| `--upload-repo-file` | flag | also upload the versioned RPM tree |

**Returns:** The `argparse.Namespace`.

**Errors:** Each missing required value is logged (`Missing R2 ...`), usage is printed and `exit(1)` is called.

**Special concerns:** Reads `sys.argv` and `os.environ`. Four of the settings are secrets; prefer the environment over flags (flags show in `ps`).

**Signature:** `def parse_args()`

**See also:** `test_the_required_values_are_returned_with_the_default_distributions_and_architectures`, `test_values_may_come_from_the_environment`, `test_each_missing_required_value_exits_with_status_1`.



## Data structures of the release scripts

The classes of the three upstream release scripts and of the test support `release_script_testing.py` (ADR 0113). They run in Cloudflare's release pipeline (the scripts) or in unit tests (the support module); nothing here transmits and nothing runs in the relay or on `hams1`.

### `PkgUploader` [@ANCHOR: release_pkgs:PkgUploader]

**Purpose and location.** Uploads one built package file to a Cloudflare R2 bucket. `daemons/cloudflared/release_pkgs.py`, a plain class created once by the script's `main` from its command-line options.

**Definition.** `class PkgUploader` with `__init__(account_id, bucket_name, client_id, client_secret)` and `upload_pkg_to_r2(filename, upload_file_path)`.

| Attribute | Type | Meaning |
|---|---|---|
| `account_id` | `str` | Cloudflare account id; the endpoint is `https://<account_id>.r2.cloudflarestorage.com`. |
| `bucket_name` | `str` | The R2 bucket the file goes into. |
| `client_id` | `str` | R2 access key id (an S3-style credential). |
| `client_secret` | `str` | R2 secret access key. A secret: never logged by this class; it comes from the pipeline's environment. |

**Invariants and lifetime.** Plain attribute holder, no validation in the class (the script's `main` refuses missing options before creating it). Never changed after construction; no concurrency concerns beyond one boto3 client created per upload.

**Serialized form.** None (in memory only).

**Functions that touch it.** `PkgUploader.__init__`, `PkgUploader.upload_pkg_to_r2` (see their entries above). Tests: `test_release_pkgs.py`.

### `PkgCreator` [@ANCHOR: release_pkgs:PkgCreator]

**Purpose and location.** Builds the signed apt and yum repositories from the packages: writes the reprepro distribution file, runs reprepro for each `.deb`, builds the rpm repository, signs it and writes the `.repo` file. `daemons/cloudflared/release_pkgs.py`.

**Definition.** `class PkgCreator` with no attributes and no `__init__`: a namespace of methods (`create_distribution_conf`, `create_deb_pkgs`, `create_rpm_pkgs`, `create_repo_file`, `import_gpg_keys` and the private `_sign_rpms`, `_sign_repomd`, `_setup_rpm_pkg_directories`).

**Invariants and lifetime.** Stateless: an instance carries nothing between calls, so the order of calls is the caller's (the script's `main`) and every method gets its inputs as arguments. The methods run external programs (reprepro, createrepo, gpg) and raise on a non-zero exit.

**Serialized form.** The files the methods write (`conf/distributions`, the repository directories, `.repo` file) are documented in the method entries above.

**Functions that touch it.** All methods named above. Tests: `test_release_pkgs.py`.

### `FakeResponse` [@ANCHOR: release_script_testing:FakeResponse]

**Purpose and location.** Just enough of `requests.Response` for the tests of `github_message.py` and `github_release.py`: `daemons/cloudflared/release_script_testing.py`.

| Attribute | Type | Meaning |
|---|---|---|
| `status_code` | `int` | HTTP status, default 200. |
| `text` | `str` | Body text, default empty. |
| `_json` | `object` or `None` | The value `json()` returns; `None` means "the body is not JSON". |

**Behaviour.** `json()` returns `_json`, or raises `ValueError("not JSON")` when it is `None`. No network, no state change. Tests: `test_release_script_testing.py`.

### `GithubException` [@ANCHOR: release_script_testing:GithubException]

**Purpose and location.** Stand-in for `github.GithubException`, defined inside `_install_missing_third_party_modules()` in `daemons/cloudflared/release_script_testing.py` and registered in `sys.modules["github"]` only when PyGithub is not installed (an installed package is used as it is). Lets the release scripts be imported on a box without PyGithub.

| Attribute | Type | Meaning |
|---|---|---|
| `status` | `int` | HTTP status, default 0. |
| `data` | `dict` | Response data, default `{}` (a `None` argument becomes `{}`). |

**Notes.** `args` are `(status, data)`. Lifetime: the process (it lives in `sys.modules`). Tests: `test_release_script_testing.py`, which hides the real package and restores `sys.modules`.

### `UnknownObjectException` [@ANCHOR: release_script_testing:UnknownObjectException]

**Purpose and location.** Stand-in for `github.UnknownObjectException`, the "not found" error the release scripts catch when a release or file is missing. Same function, same `sys.modules["github"]` registration, only when PyGithub is not installed.

**Definition.** A subclass of the stand-in `GithubException` with no body, so it inherits `status` and `data` and `except GithubException` also catches it, as with the real package. Tests: `test_release_script_testing.py`.

### `Github` [@ANCHOR: release_script_testing:Github]

**Purpose and location.** Stand-in for `github.Github`, the PyGithub client, registered in `sys.modules["github"]` only when PyGithub is not installed (in `release_script_testing.py`).

**Behaviour.** Its constructor takes any arguments and always raises `AssertionError("a test must replace Github with a mock; no real GitHub call")`, so no test can reach the real GitHub API by accident through the stand-in. A test that needs a client patches the script's `Github` name with a mock. Tests: `test_release_script_testing.py`.

### `Config` [@ANCHOR: release_script_testing:Config]

**Purpose and location.** Stand-in for `botocore.client.Config`, registered in `sys.modules["botocore.client"]` only when botocore is not installed (in `release_script_testing.py`); `release_pkgs.py` builds its R2 client configuration with it.

| Attribute | Type | Meaning |
|---|---|---|
| `kwargs` | `dict` | The keyword arguments the constructor was given (`region_name`, `s3` addressing style), kept so a test can read them. |

**Notes.** It does nothing else: the boto3 client is replaced by a mock in the tests. Tests: `test_release_script_testing.py`.

### `ClientError` [@ANCHOR: release_script_testing:ClientError]

**Purpose and location.** Stand-in for `botocore.exceptions.ClientError`, registered in `sys.modules["botocore.exceptions"]` only when botocore is not installed (in `release_script_testing.py`).

**Definition.** A bare `Exception` subclass with no attributes. `PkgUploader.upload_pkg_to_r2` catches `ClientError` and re-raises it, which `test_release_pkgs.py` checks with this class. Tests: `test_release_script_testing.py`.

### `GPG` [@ANCHOR: release_script_testing:GPG]

**Purpose and location.** Stand-in for `gnupg.GPG` (python-gnupg), registered in `sys.modules["gnupg"]` only when python-gnupg is not installed (in `release_script_testing.py`).

**Behaviour.** Its constructor always raises `AssertionError("a test must replace gnupg.GPG with a mock; no real gpg call")`, so no GPG key import or signing can happen through the stand-in; the key-import tests patch the script's `gnupg.GPG` with a mock. The module's `boto3.client` stand-in is a function that raises the same way. Tests: `test_release_script_testing.py`.
