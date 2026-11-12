# Requirements
1. Python 3.10 or later with packages in the given `requirements.txt`
   - E.g. with venv:
   - `python3 -m venv ./.venv`  
   - `source ./.venv/bin/activate`
   - `python3 -m pip install -r requirements.txt`

2. Create a config yaml file, for example:
```
cloudflared_binary: "cloudflared"
tunnel: "3d539f97-cd3a-4d8e-c33b-65e9099c7a8d"
credentials_file: "/Users/tunnel/.cloudflared/3d539f97-cd3a-4d8e-c33b-65e9099c7a8d.json"
origincert: "/Users/tunnel/.cloudflared/cert.pem"
ingress:
- hostname: named-tunnel-component-tests.example.com
  service: hello_world
- service: http_status:404
```

3. Route hostname to the tunnel. For the example config above, we can do that via
```
   cloudflared tunnel route dns 3d539f97-cd3a-4d8e-c33b-65e9099c7a8d named-tunnel-component-tests.example.com
```

4. Turn on linter
If you are using Visual Studio, follow https://code.visualstudio.com/docs/python/linting to turn on linter.

5. Turn on formatter
If you are using Visual Studio, follow https://code.visualstudio.com/docs/python/editing#_formatting
to turn on formatter and https://marketplace.visualstudio.com/items?itemName=cbrevik.toggle-format-on-save
to turn on format on save.

6. If you have cloudflared running as a service on your machine, you can either stop the service or ignore the service tests
via `--ignore test_service.py`

# How to run
Specify path to config file via env var `COMPONENT_TESTS_CONFIG`. This is required.
## All tests
Run `pytest` inside this(component-tests) folder

## Specific files
Run `pytest <file 1 name>.py <file 2 name>.py`

## Specific tests
Run `pytest file.py -k <test 1 name> -k <test 2 name>`

## Live Logging
Running with `-o log_cli=true` outputs logging to CLI as the tests are. By default the log level is WARN.
`--log-cli-level` control logging level.
For example, to log at info level, run `pytest -o log_cli=true --log-cli-level=INFO`.
See https://docs.pytest.org/en/latest/logging.html#live-logs for more documentation on logging.

# Quick tunnel readiness

A quick tunnel's hostname is not reachable the moment cloudflared prints it, so the quick-tunnel component tests wait for it with two helpers in `util.py`.

### `wait_quicktunnel_ready` [@ANCHOR: cloudflared:wait_quicktunnel_ready]

Gives a fresh quick tunnel's public hostname time to appear in DNS and then waits until it answers. It sleeps `DNS_BACKOFF_SECS` (15 seconds, from `constants.py`) first, because the name is created by Cloudflare when the tunnel starts and resolvers hold a negative answer for a while, then calls `_wait_quicktunnel_ready`. The tests `test_quick_tunnel` and `test_quick_tunnel_url` call it after `get_quicktunnel_url()`.

| Argument | Type | Meaning |
|---|---|---|
| `url` | str | full URL to probe, for example `https://<name>.trycloudflare.com`, or that URL plus `/ready` when the origin is the metrics server |

**Returns:** Nothing.

**Errors:** Whatever `_wait_quicktunnel_ready` raises once its attempts are used up (an `AssertionError` for a non-200 answer or a `requests` exception).

**Special concerns:** Blocks the test thread for at least 15 seconds with a plain `sleep`, then up to the retry time of the second function. Network only: HTTPS GETs to the public internet through Cloudflare's edge, so it needs a working uplink and a running `cloudflared` started by the test. No files, no locks.

**Signature:** `def wait_quicktunnel_ready(url)`

**See also:** `test_quick_tunnel`, `test_quick_tunnel_url` in `test_quicktunnels.py`; `cloudflared:_wait_quicktunnel_ready`.
### `_wait_quicktunnel_ready` [@ANCHOR: cloudflared:_wait_quicktunnel_ready]

Probes a URL until it answers HTTP 200. It is wrapped in `retrying.retry` with `MAX_RETRIES` (5) attempts and a fixed `BACKOFF_SECS` (7 seconds) wait between them. Each attempt is a new `requests.Session` GET with a timeout of `DNS_BACKOFF_SECS` (15 seconds), and anything but status 200 fails the attempt.

| Argument | Type | Meaning |
|---|---|---|
| `url` | str | URL to request |

**Returns:** Nothing on success.

**Errors:** After the fifth failed attempt the last exception propagates: an `AssertionError` reading `<url> returned <response>` for a non-200 answer, or a `requests` connection or timeout error.

**Special concerns:** Worst case about 5 x 15 s of timeouts plus 4 x 7 s of waiting. Blocking, single-threaded, network only; the session is closed on exit. Do not call it directly from tests; use `wait_quicktunnel_ready`, which adds the DNS delay.

**Signature:** `def _wait_quicktunnel_ready(url)` (decorated with `@retry(stop_max_attempt_number=MAX_RETRIES, wait_fixed=BACKOFF_SECS * 1000)`)

**See also:** `test_quick_tunnel`, `test_quick_tunnel_url`; `cloudflared:wait_quicktunnel_ready`.


## Data structures of the component tests

The classes the component tests are built from (ADR 0113). They need a real Cloudflare account and tunnel and so only run in Cloudflare's own CI; `config.py` is pure data and is also unit-tested here (`../test_component_config.py`). Never run on the relay hosts.

### `CloudflaredCli` [@ANCHOR: cloudflared:CloudflaredCli]

**Purpose and location.** Drives the `cloudflared tunnel ...` command line for a test: lists tunnels, gets tunnel and connector info, fetches management and tail tokens, and (as a context manager) runs a tunnel. `component-tests/cli.py`.

**Definition.** `class CloudflaredCli`, built as `CloudflaredCli(config, config_path, logger)`.

| Attribute | Type | Meaning |
|---|---|---|
| `basecmd` | `list[str]` | `[config.cloudflared_binary, "tunnel"]`, plus `--config <config_path>` when given and `--origincert <cert>` when the component-tests config has an `origincert`. Extended by `__enter__` with `run`. |
| `logger` | `logging.Logger` | Where command output is logged at debug level. |
| `cfd` | `CloudflaredProcess` | Set by `__enter__` only: the running tunnel process, cleaned up by `__exit__`. |

**Behaviour and errors.** Each helper runs the command with a 15 s timeout (`subprocess.TimeoutExpired` propagates) and a non-zero exit raises `SubprocessError`. Output of `list` and `info` is parsed as JSON. Used as `with CloudflaredCli(...) as cfd:` it starts `tunnel run` and terminates it on exit.

### `SubprocessError` [@ANCHOR: cloudflared:SubprocessError]

**Purpose and location.** The exception `run_subprocess` raises when a command exits non-zero. `component-tests/cli.py`.

| Attribute | Type | Meaning |
|---|---|---|
| `program` | `str` | The program that failed (the first word of the command). |
| `exit_code` | `int` | Its exit status. |
| `cause` | `subprocess.CalledProcessError` | The original exception, with stdout and stderr. |

A timeout is not wrapped: `subprocess.TimeoutExpired` propagates as it is.

### `BaseConfig` [@ANCHOR: cloudflared:BaseConfig]

**Purpose and location.** Settings common to every cloudflared test run: `component-tests/config.py`, a frozen dataclass (assigning to a field raises, which emulates immutability).

| Field | Type | Meaning |
|---|---|---|
| `cloudflared_binary` | `str` | Path of the binary under test (required). |
| `no_autoupdate` | `bool` | Default True; written to the config as `no-autoupdate`. |
| `metrics` | `str` | Default `localhost:<METRICS_PORT>`; written as `metrics`. |

`merge_config(additional)` returns a copy of `additional` with `no-autoupdate` and `metrics` set from the object.

### `NamedTunnelBaseConfig` [@ANCHOR: cloudflared:NamedTunnelBaseConfig]

**Purpose and location.** The settings of a named (account) tunnel: `component-tests/config.py`, frozen dataclass derived from `BaseConfig`.

| Field | Type | Meaning |
|---|---|---|
| `tunnel` | `str` | Tunnel id or name (required: `TypeError` when missing). |
| `credentials_file` | `str` | Path of the tunnel credentials JSON (required). |
| `ingress` | `list` | The ingress rules (required; may be an empty list). |
| `hostname` | `str` | Hostname routed to the tunnel (default `None`). |

The fields default to `None` only because the parent's defaults come first; `__post_init__` raises `TypeError` for a missing `tunnel`, `credentials_file` or `ingress`. `merge_config` adds `tunnel`, `credentials-file` and `ingress` unless `additional` already has them (config tests override the ingress).

### `NamedTunnelConfig` [@ANCHOR: cloudflared:NamedTunnelConfig]

**Purpose and location.** The ready-to-use configuration of a named tunnel test: `component-tests/config.py`, frozen dataclass derived from `NamedTunnelBaseConfig`, built by the `component_tests_config` fixture.

| Field | Type | Meaning |
|---|---|---|
| `full_config` | `dict` | The merged configuration written to the config file (set in `__post_init__` with `object.__setattr__`, since the class is frozen). |
| `additional_config` | `InitVar[dict]` | Constructor-only: keys that override the defaults; not stored. |

Methods: `get_url()` (`https://<hostname>`), `base_config()` (a copy of `full_config` without `tunnel` and `credentials-file`), `get_tunnel_id()`, `get_credentials_json()` (reads the credentials file) and `get_token()` (the base64 of `{"a": AccountTag, "t": TunnelID, "s": TunnelSecret}`, the token form `cloudflared tunnel run --token` takes). The credentials hold the tunnel secret: the tests keep them in a temporary directory and never log them.

### `QuickTunnelConfig` [@ANCHOR: cloudflared:QuickTunnelConfig]

**Purpose and location.** The configuration of a quick-tunnel (account-less) test: `component-tests/config.py`, frozen dataclass derived from `BaseConfig` with `full_config` (the merged configuration) and the constructor-only `additional_config`, merged exactly as in `NamedTunnelConfig` but without any tunnel or credential fields. The quick tunnel needs no account, so only the base keys (`no-autoupdate`, `metrics`) plus the caller's `additional_config` (for example `url`) end up in `full_config`. Built by the fixture for `CfdModes.QUICK`; unit-tested in `../test_component_config.py`.

### `CfdModes` [@ANCHOR: cloudflared:CfdModes]

**Purpose and location.** The two tunnel modes the `component_tests_config` fixture can build: `component-tests/conftest.py`, an `enum.Enum` with `NAMED` (a `NamedTunnelConfig`) and `QUICK` (a `QuickTunnelConfig`), values from `auto()`. Any other value raises `Exception("Unknown cloudflared mode ...")`. Note: `test_service.py` still passes `CfdModes.CLASSIC`, which does not exist in this enum (unknown whether upstream removed it; those service tests are not run by us).

### `CloudflaredProcess` [@ANCHOR: cloudflared:CloudflaredProcess]

**Purpose and location.** A wrapper around a `subprocess.Popen` that keeps draining the child's output in a background thread so a full OS pipe never blocks cloudflared, and logs the captured output at cleanup. `component-tests/util.py`, created by `start_cloudflared` and `CloudflaredCli.__enter__`.

| Attribute | Type | Meaning |
|---|---|---|
| `process` | `subprocess.Popen` | The child; stdout and stderr merged (`STDOUT`), or discarded when not captured; stdin a pipe only when `allow_input`. |
| `_capture_output` | `bool` | Whether output is captured. |
| `_stdout_lines` | `list[bytes]` | Lines read so far, exposed as `stdout_lines`. |
| `_threads` | `list[Thread]` | The daemon reader threads, joined at cleanup (timeout `READER_THREAD_JOIN_TIMEOUT`). |

**Behaviour.** `terminate()` terminates a running child; `cleanup()` terminates it, waits `GRACEFUL_SHUTDOWN_TIMEOUT` seconds then kills it, joins the readers and logs the output. Other attributes (`pid`, `returncode`, `send_signal`, `stdin`) are proxied to the `Popen` through `__getattr__`. The reader threads append to a shared list without a lock (appending is atomic in CPython and only one thread writes).
