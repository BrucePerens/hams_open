<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Model Method Is Callable Over RPC Only If Someone Listed It

As the **Operator** of an Odoo server running many addons, I want a method that nobody meant to expose to be
unreachable from the browser's `call_kw`, from JSON-2 and from XML-RPC, without reading every addon, so that the next
public method that returns a secret or skips a check is refused by default instead of found in a bug hunt.

## What is judged

Every call that arrives over `/web/dataset/call_kw`, `/web/dataset/call_button`, `/json/2/...`, `/xmlrpc/2/object` or
`/jsonrpc` passes `odoo.service.model.get_public_method`, after Odoo's own refusal of private names and `@api.private`.
A call from Python code (a cron, a daemon's own model code, a test) has no such request and is never judged.

The story, step by step. Each function that makes it true has its own entry below, grouped by source file.

1. The guard is installed once when the module is loaded, over `odoo.service.model.get_public_method`, `execute_cr` and
   the name the JSON-2 controller imported.
2. A call is "RPC" when `execute_cr` marked it (XML-RPC and JSON-RPC run with the HTTP request taken off the thread),
   or its request path is one of the browser or JSON-2 paths.
3. The facts about the call are gathered: the user's class (public, portal, service account, internal), the service
   account's login, every class that defines the method, whether all of them belong to our addons, whether the name is a
   standard ORM method, and the `@public_rpc` marks.
4. The decision follows fixed rules, in order.
   * (A service account is recognised first: most of ours are portal-type users, and a daemon must never be judged as a
     portal visitor.)
   * A portal or public user may call only a pair declared `portal` (or a method marked `@public_rpc(portal=True)`).
     This applies to stock models too.
   * A service account with a `clients` entry may call only the pairs in its entry, so a compromised daemon cannot call
     the methods of another daemon's family.
   * An internal user may call a method of ours only if it is declared `internal` or marked. A stock method or a
     standard ORM override (`write`, `search_read`, `web_read`, ...) keeps core behaviour, unless `strict_stock` asks
     for those to be declared too.
5. `enforce` refuses with an access error; `report` serves the call and counts it; `off` judges nothing. A refusal or a
   would-be refusal is counted per worker and logged at the first and every power of ten with the user class,
   `model.method` and the reason; never an argument, record id, login or address.

## Daemons off the Odoo host: mutual TLS

For a daemon on another machine, the operator terminates TLS at nginx with `ssl_verify_client on;`, one client
certificate per daemon, and passes the verified subject in a header that nginx sets itself (`proxy_set_header
X-Client-Cert-Subject $ssl_client_s_dn;`, which replaces anything the client sent). Setting
`hams_rpc_guard.client_cert_header` to that header name and listing the proxy's loopback address in
`hams_rpc_guard.trusted_proxies` makes the subject the daemon's second identity, `cert:<subject>`, and `clients` may
have an entry under that key. When both the login and the certificate have an entry the pair must be in both, so the
header can only narrow a daemon, never widen it.

## How an addon declares what it exposes

* In code: `@public_rpc` above a method (`@public_rpc(portal=True)`, `@public_rpc(clients=("login",))`). The decorator
  lives in `zero_sudo` (`from odoo.addons.zero_sudo import public_rpc`), so an addon needs no dependency on this one.
* In a file: `rpc_exposure.json` in the addon's root. The installed addons' files are merged once per registry.
* The transitional list (`internal`) of every addon of a repository is generated from its source, never by hand:
  `python3 hams_rpc_guard/exposure_scan.py --write .` (check: `--check .`). A test fails when a new public method is on
  neither the list nor the decorator.
* A daemon's entry names its source files; a test reads them and requires every model and method the daemon calls to
  be in the entry, so a daemon keeps working when its code changes only together with its allowlist. The scan also reads
  the batch helper (`push_batches_to_odoo(client, logger, rows, "model", "method")`) and a caller passed along
  (`asyncio.to_thread(client.execute, "model", "method")`): those are the daemons' main calls.
* In report mode the operator reads the guard's log lines (one at the 1st, 10th, 100th, ... would-be refusal of each kind per
  worker); `tools/report_summary.py` totals them over the workers, so a few days of log become a short table of what to list or fix
  before switching to enforce.

## `hams_rpc_guard/hooks.py`

### `post_load_hook()` [@ANCHOR: hams_rpc_guard:post_load_hook]

Runs when the module is loaded as a server-wide module, before any request is served and, under `--workers`, in the master before any worker is forked. Takes no arguments, returns nothing. It calls `patch.install()` (the method guard) and `unix_listener.install_server_patches()` (the Unix-socket server hooks, which open no socket unless the server configuration names one). Idempotent: loading twice wraps nothing twice. Side effects: replaces functions in `odoo.service.model` and `odoo.addons.rpc.controllers.json2`, and methods of Odoo's server classes. Tests: `tests/test_guard.py` `test_the_module_is_installed_over_the_core_functions`.

## `hams_rpc_guard/patch.py`

### `install()` [@ANCHOR: hams_rpc_guard:install]

Replaces three names once: `odoo.service.model.get_public_method` and `execute_cr` with the guarded versions, and the copy of `get_public_method` that the JSON-2 controller imported (`json2.get_public_method`) with the same guarded function. Each replacement is skipped if it already carries the `_hams_rpc_guard_wrapped` marker, so it is idempotent. No arguments, no return value, no lock (called once at load, before any request). Security: this single choke point covers `call_kw`, `call_button`, XML-RPC, JSON-RPC and JSON-2 together. Tests: `test_the_module_is_installed_over_the_core_functions`.

### `guard_get_public_method(original)` [@ANCHOR: hams_rpc_guard:guard_get_public_method]

Returns the replacement for `odoo.service.model.get_public_method`. Argument `original`: the core function, which still runs first, so Odoo's own refusals (private names, `@api.private`) are unchanged and come before the guard. The wrapper `guarded(model, name)` then, only when `call_is_rpc()` is true, calls `check_rpc_call(model, name)`, which may raise `AccessError`, and returns the original method. Python callers are never judged. Marked with `_hams_rpc_guard_wrapped`. Tests: `test_core_refusals_still_come_first`, `test_python_code_is_never_judged`.

### `guard_execute_cr(original)` [@ANCHOR: hams_rpc_guard:guard_execute_cr]

Returns the replacement for `odoo.service.model.execute_cr`. Argument `original`: the core function. The wrapper sets the context variable `hams_rpc_guard_external` for the duration of the call and resets it in a `finally`, so everything inside, including `get_public_method`, is marked as an external RPC. This is needed because XML-RPC and JSON-RPC run `execute_cr` with the HTTP request removed from the thread, so the path alone cannot identify them. A context variable is per task and per thread, so concurrent calls never mark each other. Tests: `test_xmlrpc_execute_kw_is_judged_too`.

### `call_is_rpc() -> bool` [@ANCHOR: hams_rpc_guard:call_is_rpc]

Whether the call being served arrived over RPC. No arguments. True when `execute_cr` set the external marker, or when the current request path begins with `/web/dataset/call_kw` or `/web/dataset/call_button` or is a JSON-2 path (`endpoint_gate.endpoint_family`). False when there is no request on the thread (cron, a daemon's own model code, a test) or no path. A `RuntimeError` from reading the request is treated as no request. Pure apart from reading thread state. Tests: `test_python_code_is_never_judged`, `test_an_internal_user_keeps_the_web_client`.

### `_module_is_managed(module, authors) -> bool` [@ANCHOR: hams_rpc_guard:module_is_managed]

Whether an addon is one of ours. Arguments: `module` (addon name) and `authors` (the tuple of author substrings). It reads the addon's manifest through `exposure.manifest_of` and returns True when a manifest exists and its `author` contains any of the substrings (`policy.author_is_managed`). Cached with `functools.lru_cache(maxsize=4096)` keyed on both arguments, so a changed `managed_authors` parameter produces a fresh answer while an unchanged one costs nothing. Thread-safe through the cache. Tests: `tests/test_guard.py` managed-method tests.

### `describe_call(model, name, authors, exposure, cert="") -> policy.Call` [@ANCHOR: hams_rpc_guard:describe_call]

Gathers the facts about one call. Arguments: `model` (the recordset), `name` (method name), `authors`, the merged `Exposure`, and `cert` (the identity string from the Unix socket or client certificate, or ""). It classifies `env.user` as public, service account (tested before portal, because most service accounts are portal-type users), portal or internal; lists every class in the MRO that defines the method; decides `managed` only if every defining class belongs to our addons (an override of a stock method keeps stock behaviour); and collects `@public_rpc` marks. Returns a `policy.Call`. Reads the database for the user class only. Tests: `tests/test_guard.py`.

### `Call.__init__(self, model, method, user_class, login="", defining_models=(), managed=False, orm_standard=False, marks=(), cert="")` [@ANCHOR: hams_rpc_guard:call_init]

Constructor of `policy.Call`, the plain record of the facts about one call so a test can build it without Odoo. `model` and `method` are names; `user_class` one of `public`, `portal`, `service`, `internal`; `login` the service account's login; `defining_models` the `_name` of every class defining the method (converted to a tuple); `managed` whether all defining classes are ours; `orm_standard` whether the name is a standard ORM method; `marks` the `@public_rpc` marks; `cert` the second identity. Stores only; no I/O. Tests: `tests/test_policy.py`.

### `check_rpc_call(model, name)` [@ANCHOR: hams_rpc_guard:check_rpc_call]

Judges one RPC call and refuses or counts it. Arguments: the recordset `model` and the method `name`. It first reads the Unix-socket identity (`unix_identity`, from the environ alone, so an unreadable parameter cannot drop the login binding), then the mode, `strict_stock` and `managed_authors` parameters and the certificate identity through `zero_sudo.security.utils`; if any read fails it logs and uses the strict values (enforce, strict_stock off, default authors). Mode `off` returns. Otherwise it describes the call, applies the Unix login binding (a login not in the bound set is denied as `unix-login-not-bound`), calls `policy.decide`, and `policy.apply_verdict`, which may raise `AccessError`. Tests: `test_an_unreadable_parameter_fails_closed_to_enforce`, `test_a_login_that_is_not_bound_is_refused_and_report_mode_only_counts`.

### `certificate_identity(utils) -> str` [@ANCHOR: hams_rpc_guard:certificate_identity]

Returns `cert:<subject>` when the operator named a header in `hams_rpc_guard.client_cert_header` and the request carries a non-empty value for it from a trusted proxy, otherwise "". Argument `utils`: a `zero_sudo.security.utils` recordset used to read parameters. The header is believed only when `peer_is_trusted` holds for the request's socket peer and `trusted_proxies`. Security: the identity can only narrow a daemon (`policy.decide`), so a local process that forges the header gains nothing. Returns "" with no request. Tests: `test_a_certificate_subject_is_believed_only_from_a_trusted_proxy`.

## `hams_rpc_guard/endpoint_gate.py`

### `peer_is_trusted(environ, trusted_proxies) -> bool` [@ANCHOR: hams_rpc_guard:peer_is_trusted]

True when the socket peer of the request is a loopback address inside one of `trusted_proxies`. Arguments: the WSGI `environ` and the tuple of trusted networks. It uses `socket_peer` (the address before any `proxy_mode` rewriting). Stricter than `resolve_client`, which also trusts a non-loopback proxy for the client address: the client-certificate header is believed only from a proxy on this machine. Pure, no I/O. Tests: `tests/test_endpoint_gate.py` `test_peer_is_trusted_needs_a_loopback_peer_inside_the_list`.

## `hams_rpc_guard/policy.py`

### `_declared(pairs, call) -> bool` [@ANCHOR: hams_rpc_guard:declared]

Whether the call's pair is in a set of `(model, method)` pairs. Arguments: `pairs` and a `Call`. The pair counts under the model's own name or under the name of any class that defines the method, so a method listed under a mixin's name admits every model that inherits it. Pure, no I/O. Used by `decide` for the portal, client, internal and stock lists. Tests: `tests/test_policy.py` `test_a_pair_listed_under_the_mixin_name_counts`.

### `decide(call, exposure, strict_stock=False) -> (str, str)` [@ANCHOR: hams_rpc_guard:decide]

The verdict on one call: `(ALLOW | DENY, reason)`, `reason` a short constant used by the counters. Order: public or portal users are allowed only a `portal`-declared or `portal`-marked pair (`strict-public`, `strict-portal` otherwise); a service account with a `clients` entry under its certificate or login must be listed under every entry it has (`client-listed`, `client-not-listed`); a managed non-ORM method needs an `internal` listing or a mark (`listed`, `managed-not-listed`); with `strict_stock` a stock method needs `stock` or `internal`; everything else is `core`. A service account with no entry is judged as internal, with `unmapped-client-` reasons. Pure, no I/O. Tests: `tests/test_policy.py`.

### `normalise_mode(value) -> str` [@ANCHOR: hams_rpc_guard:normalise_mode]

Reads the `hams_rpc_guard.mode` parameter. Argument `value`: the stored text. Returns `enforce`, `report` or `off` (trimmed, lower case); unset, blank or anything unrecognised is `enforce`, so a typo fails closed: an operator who writes `enforse` still gets enforcement, never silence. Used by `check_rpc_call` and by the endpoint gate. Pure, never raises, no I/O. Tests: `test_mode_defaults_and_fails_closed`.

### `truthy(value) -> bool` [@ANCHOR: hams_rpc_guard:truthy]

Whether a stored parameter means yes: `1`, `true`, `yes` or `on`, in any case, after trimming. Argument `value`: any value (None and False read as no). Used for `hams_rpc_guard.strict_stock`. Anything else, including junk, is False, so the less strict reading is the default for this one switch. Pure, never raises. Tests: `test_truthy_and_authors`.

### `parse_authors(value) -> tuple` [@ANCHOR: hams_rpc_guard:parse_authors]

The author substrings that mark a manifest as one of ours. Argument `value`: the stored `hams_rpc_guard.managed_authors` (comma-separated). Returns the trimmed non-empty items as a tuple, or the defaults (`Bruce Perens`, `Hams`, `HAMS`) when nothing usable is given. Pure, never raises. Security: a method of an addon whose author matches is "managed" and so needs listing for internal users; an addon whose author does not match keeps core behaviour. Tests: `test_truthy_and_authors`.

### `author_is_managed(author, authors) -> bool` [@ANCHOR: hams_rpc_guard:author_is_managed]

Whether a manifest `author` string contains any of the substrings in `authors`, compared case-insensitively. Arguments: `author` (None reads as empty) and `authors` (the tuple from `parse_authors`). Returns False for an empty author. Pure, never raises. Called through the cached `_module_is_managed`. Tests: `tests/test_guard.py`.

### `record_verdict(call, verdict, reason, mode) -> int` [@ANCHOR: hams_rpc_guard:record_verdict]

Counts a denial and logs it. Arguments: the `Call`, `verdict`, `reason` and `mode`. Allowed calls return 0 and are not counted. A denial increments a per-worker counter keyed by mode, user class, `model.method` and reason under a lock, and is logged at WARNING at the first and every power of ten, adding that the call was served in report mode. Only names are logged: never an argument, record id, login or address. Returns the count. Tests: `test_allowed_calls_are_not_counted_and_the_log_is_at_powers_of_ten`.

### `verdict_counts() -> dict` [@ANCHOR: hams_rpc_guard:verdict_counts]

A copy of this worker's counters, `{(mode, user_class, "model.method", reason): count}`, taken under the counter lock. No arguments, no side effects. Counters are per process and start again when a worker restarts. `tools/report_summary.py` reads the corresponding log lines to add them over workers. Tests: `test_report_mode_counts_and_serves`.

### `apply_verdict(call, verdict, reason, mode)` [@ANCHOR: hams_rpc_guard:apply_verdict]

Records the verdict (`record_verdict`) and, in enforce mode, raises `odoo.exceptions.AccessError` for a denied call. Arguments: the `Call`, `verdict`, `reason`, `mode`. In report mode a denial is counted and the call goes on; an allow does nothing. The message is the same for every refusal, "The method 'model.method' cannot be called remotely.", so it tells a prober no more than Odoo's own message. Tests: `test_enforce_mode_refuses_with_an_access_error_that_names_only_the_pair`.

## `zero_sudo/rpc_public.py`

### `public_rpc(func=None, *, portal=False, clients=())` [@ANCHOR: zero_sudo:public_rpc]

The decorator an addon uses to declare a model method callable over RPC. Bare `@public_rpc` admits any signed-in internal user; `portal=True` also admits portal and public users; `clients=("login", ...)` also admits those service-account logins. It only sets an attribute on the function (`{"portal": bool, "clients": tuple}`) and returns the same function, so it adds no wrapper and no cost per call. It lives in `zero_sudo` so an addon needs no dependency on `hams_rpc_guard`. Place it above `@api.model` and below nothing that wraps the function in a new object. Tests: `tests/test_exposure.py` `test_the_decorator_returns_the_function_itself`.

## `hams_rpc_guard/exposure.py`

### `decorator_marks(model_class, name) -> list` [@ANCHOR: hams_rpc_guard:decorator_marks]

The `@public_rpc` marks on `name` in every class of `model_class`'s method resolution order that defines it, as a list of dicts. Arguments: the model class and the method name. Only plain functions carrying the mark attribute count. Pure, no I/O. An override that lacks the decorator does not hide the mark on a parent, because every class in the MRO is checked. Tests: `test_marks_are_found_through_the_mro`.

### `Exposure.__init__(self)` [@ANCHOR: hams_rpc_guard:exposure_init]

Creates an empty registry of declarations: sets of `(model, method)` pairs `internal`, `portal` and `stock`, `clients` (a dict from login or `cert:`/`unix:` identity to its set of pairs), `sources` (a dict from a client to its daemon source files) and `modules` (the addons that contributed a file). No arguments, no I/O. One instance is built per registry by `load_exposure` and then only read, so it needs no lock. Tests: `test_merge_reads_every_section_and_ignores_junk`.

### `Exposure.merge(self, document, origin)` [@ANCHOR: hams_rpc_guard:exposure_merge]

Adds one parsed `rpc_exposure.json` document. Arguments: `document` (a dict) and `origin` (addon name, for log messages). Sections `internal`, `portal`, `stock` are unioned in; `clients` entries (either a list of pairs or `{"allow": ..., "sources": [...]}`) are unioned per login and their source files appended. Unknown keys log a warning; a `clients` value that is not a mapping is logged and ignored rather than failing every RPC call. Malformed pairs are skipped. Tests: `test_merge_reads_every_section_and_ignores_junk`, `test_a_clients_section_that_is_not_a_mapping_is_ignored_not_fatal`.

### `_pairs(section) -> set` [@ANCHOR: hams_rpc_guard:pairs]

The `(model, method)` pairs of a `{"model.name": ["method", ...]}` mapping. Argument `section`: anything. Anything that is not a dict, a model whose methods are not a list or tuple, and any method that is not a string is skipped silently, so a hand-edited file with a mistake loses entries rather than raising. Skipped entries mean a method is unknown to the guard and is denied in enforce mode, never allowed. Pure. Tests: `test_merge_reads_every_section_and_ignores_junk`.

### `load_exposure(cr, registry) -> Exposure` [@ANCHOR: hams_rpc_guard:load_exposure]

Returns the merged `Exposure` of the registry's installed addons, read once per registry and cached on the registry object (a module change builds a new registry, which starts empty). Arguments: `cr` (any cursor, used once to list installed modules) and the `registry`. For each installed addon with an `rpc_exposure.json` it loads the JSON and merges it; an unreadable file or invalid JSON is logged and skipped, and a non-dict document is ignored. Reads the filesystem and the database once; guarded by a lock for the cache. Tests: `test_the_installed_addons_files_are_merged_once_per_registry`.

### `manifest_of(module) -> manifest | None` [@ANCHOR: hams_rpc_guard:manifest_of]

The manifest of an addon, a mapping with `author` and so on, or None when it has none. Argument `module`: the addon name. Delegates to Odoo's `Manifest.for_addon` without a missing-module warning. Used by `_module_is_managed` to compare the author with the managed list. Reads the filesystem (cached by Odoo). Tests: `test_the_manifest_of_a_module`.

## `hams_rpc_guard/exposure_scan.py`

### `main(argv=None) -> int` [@ANCHOR: hams_rpc_guard:scan_main]

Command line of the generator. `python3 hams_rpc_guard/exposure_scan.py --write ROOT` rewrites each addon's `rpc_exposure.json` `internal` section from its source; `--check ROOT` only lists out-of-date addons. Argument `argv` (default `sys.argv`). For every addon under ROOT it scans the source, renders the file text with `render`, and compares it with the current one; prints "written: ..." or "out of date: ..." and returns 1 only when `--check` found stale files, else 0. It reads and writes files in the tree and needs no database. Tests: `tests/test_exposure.py` `test_an_addon_is_scanned_and_rendered_and_the_check_notices_staleness`, `tests/test_repository.py`.

### `scan_addon(path, standard=None) -> dict` [@ANCHOR: hams_rpc_guard:scan_addon]

`{model: sorted public methods}` for one addon directory. Arguments: `path` and `standard` (the set of ORM names, default from `orm_names`). It walks the tree in sorted order, skipping `tests`, `tools`, `daemons`, `static`, `docs`, migrations and hidden directories, and files starting with `test_`, parses each `.py`, and unions the methods that are public and not already marked `@public_rpc`. Reads files only; never imports the addon. Tests: `test_an_addon_is_scanned_and_rendered_and_the_check_notices_staleness`.

### `scan_source(source, standard) -> dict` [@ANCHOR: hams_rpc_guard:scan_source]

Parses one Python source text and returns `{model: {"public": set, "marked": set}}`. A method counts when it is a plain `def` or `async def` in a class that sets `_name` or `_inherit`, its name does not start with `_`, it is not a `staticmethod`, `classmethod`, `property`, `cached_property` or `@api.private`, and its name is not in `standard`. `marked` holds those already carrying `@public_rpc`. A `SyntaxError` returns an empty dict. Pure. Tests: `test_public_methods_only`.

### `model_of_class(node) -> str | None` [@ANCHOR: hams_rpc_guard:scan_model_of_class]

The model name a class body defines or extends. Argument `node`: an `ast.ClassDef`. It looks at simple assignments to `_name` and `_inherit`; returns the `_name` string, otherwise the first `_inherit` string, otherwise None (the class is not a model and is skipped). Only literal strings and lists or tuples of literals are understood; a computed name gives None. Pure. Tests: `test_public_methods_only`.

### `orm_names() -> set` [@ANCHOR: hams_rpc_guard:scan_orm_names]

The set of names defined by Odoo's `BaseModel` (`dir(BaseModel)`): the standard ORM methods such as `write`, `search_read` and `web_read` that keep core behaviour and are not listed. No arguments. Requires `odoo` to be importable, which is why the scanner runs inside an Odoo environment even though it needs no database. Pure. Tests: `test_public_methods_only`.

### `render(scanned, existing=None) -> str` [@ANCHOR: hams_rpc_guard:scan_render]

The text of an addon's `rpc_exposure.json`. Arguments: `scanned` (the `internal` mapping) and `existing` (the current document or None). Every section of `existing` except `internal` is kept, a default `comment` explaining that the list is transitional is supplied if missing, and the keys are ordered `comment`, `internal`, `portal`, `stock`, `clients`. Returns indented JSON with a trailing newline, so the output is stable and a `--check` comparison is exact. Pure. Tests: `test_an_addon_is_scanned_and_rendered_and_the_check_notices_staleness`.

### `managed_addons(root) -> list` [@ANCHOR: hams_rpc_guard:scan_managed_addons]

The addon directory names directly under `root`: every directory containing `__manifest__.py`, sorted. A repository's addons are all ours, so no author test is needed here. Argument `root`: a directory. Raises `OSError` if it cannot be listed. Pure apart from reading the directory. Tests: `tests/test_repository.py`.

### `_decorator_names(node) -> set` [@ANCHOR: hams_rpc_guard:scan_decorator_names]

The names of a method's decorators. Argument `node`: an `ast.FunctionDef`. For each decorator it takes the called function if it is a call, then the attribute name (`api.private` gives `private`) or plain name. Anything more complex is ignored. Pure. The scanner uses the result to skip static, class and private methods and to detect `@public_rpc`. Tests: `test_public_methods_only`.

### `_string_values(node) -> list` [@ANCHOR: hams_rpc_guard:scan_string_values]

The strings a class attribute holds. Argument `node`: an AST expression. A string constant gives a one-item list; a list or tuple gives its string constants in order; anything else gives an empty list. Used to read `_name` and `_inherit`. Pure. Non-literal values (a variable, a call) are not understood and are treated as no value. A tuple element that is not a string constant is dropped from the list rather than raising. Tests: `test_public_methods_only` reaches it through `model_of_class`.

### `daemon_pairs(source) -> set` [@ANCHOR: hams_rpc_guard:scan_daemon_pairs]

The `(model, method)` pairs a daemon source file calls. Argument `source`: Python text. It finds literal `/json/2/<model>/<method>` URLs, calls `x.execute`, `call`, `execute_kw`, `_json2_call`, `call_kw` or `_call` with two literal strings first, the same two strings after other arguments in `push_batches_to_odoo`, and after a caller passed along (`asyncio.to_thread(client.execute, "m", "x")`). A model or method held in a module-level string constant is resolved; names built from variables are not found. Names must look like a model (`a.b`) and a method. A `SyntaxError` returns the URL matches only. Pure. Tests: `test_daemon_pairs_from_calls_and_urls`, `test_daemon_pairs_through_a_batch_helper_and_a_passed_along_caller`.

### `_is_caller_reference(node) -> bool` [@ANCHOR: hams_rpc_guard:scan_caller_reference]

Whether an AST node is a bare reference to a calling method, such as `client.execute` passed unevaluated to `asyncio.to_thread`. Argument `node`: an attribute or name node; any other node is False. True when the attribute or name is one of the caller names (`execute`, `call`, `execute_kw`, `_json2_call`, `call_kw`, `_call`). `daemon_pairs` then reads the next two arguments as model and method. Pure.

### `module_string_constants(tree) -> dict` [@ANCHOR: hams_rpc_guard:scan_module_string_constants]

`{NAME: "text"}` for every module-level assignment of a string literal in a parsed source, including annotated assignments. Argument `tree`: an `ast.Module`. A daemon names its model (`RUN_MODEL = "ham.club.crawl.run"`) or method once and passes the name, and `daemon_pairs` resolves it through this map. Only single-target assignments of a plain string are included; other values are ignored. Pure. Tests: `test_daemon_pairs_resolve_module_level_string_constants`.

## `hams_rpc_guard/tools/report_summary.py`

### `summarise(lines) -> dict` [@ANCHOR: hams_rpc_guard:report_summary]

Totals the guard's refusal log lines over workers. Argument `lines`: an iterable of Odoo log lines. It recognises the `policy` logger's method lines and the `endpoint_gate` logger's endpoint lines and ignores everything else. Returns `{(kind, mode, what, who, reason): total}` where kind is `method` or `endpoint`; the total adds, over process ids, the highest count each logged for that key, a lower bound because a worker that counted 57 logged only the 10. A restarted worker counts again under a new pid. The command line reads files or standard input and prints a table. Pure. Tests: `tests/test_report_summary.py`.

## Verification

`tests/test_policy.py` (the decision table, modes, counters), `tests/test_exposure.py` (decorator, files, scanner),
`tests/test_guard.py` (the wrapper with real users, and through `call_kw` and XML-RPC), `tests/test_repository.py` (the
checked-in lists are current, every daemon pair is allowed).
