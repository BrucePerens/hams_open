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
