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

1. The guard is installed once when the module is loaded, over `odoo.service.model.get_public_method`, `execute_cr` and
   the name the JSON-2 controller imported. `[@ANCHOR: hams_rpc_guard:post_load_hook]`
   `[@ANCHOR: hams_rpc_guard:install]` `[@ANCHOR: hams_rpc_guard:guard_get_public_method]`
   `[@ANCHOR: hams_rpc_guard:guard_execute_cr]`
2. A call is "RPC" when `execute_cr` marked it (XML-RPC and JSON-RPC run with the HTTP request taken off the thread),
   or its request path is one of the browser or JSON-2 paths. `[@ANCHOR: hams_rpc_guard:call_is_rpc]`
3. The facts about the call are gathered: the user's class (public, portal, service account, internal), the service
   account's login, every class that defines the method, whether all of them belong to our addons, whether the name is a
   standard ORM method, and the `@public_rpc` marks. `[@ANCHOR: hams_rpc_guard:describe_call]`
   `[@ANCHOR: hams_rpc_guard:call_init]` `[@ANCHOR: hams_rpc_guard:module_is_managed]`
   `[@ANCHOR: hams_rpc_guard:check_rpc_call]`
4. The decision follows fixed rules, in order. `[@ANCHOR: hams_rpc_guard:decide]` `[@ANCHOR: hams_rpc_guard:declared]`
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
   `[@ANCHOR: hams_rpc_guard:normalise_mode]` `[@ANCHOR: hams_rpc_guard:truthy]`
   `[@ANCHOR: hams_rpc_guard:parse_authors]` `[@ANCHOR: hams_rpc_guard:author_is_managed]`
   `[@ANCHOR: hams_rpc_guard:record_verdict]` `[@ANCHOR: hams_rpc_guard:verdict_counts]`
   `[@ANCHOR: hams_rpc_guard:apply_verdict]`

## Daemons off the Odoo host: mutual TLS

For a daemon on another machine, the operator terminates TLS at nginx with `ssl_verify_client on;`, one client
certificate per daemon, and passes the verified subject in a header that nginx sets itself (`proxy_set_header
X-Client-Cert-Subject $ssl_client_s_dn;`, which replaces anything the client sent). Setting
`hams_rpc_guard.client_cert_header` to that header name and listing the proxy's loopback address in
`hams_rpc_guard.trusted_proxies` makes the subject the daemon's second identity, `cert:<subject>`, and `clients` may
have an entry under that key. When both the login and the certificate have an entry the pair must be in both, so the
header can only narrow a daemon, never widen it. `[@ANCHOR: hams_rpc_guard:certificate_identity]`
`[@ANCHOR: hams_rpc_guard:peer_is_trusted]`

## How an addon declares what it exposes

* In code: `@public_rpc` above a method (`@public_rpc(portal=True)`, `@public_rpc(clients=("login",))`).
  `[@ANCHOR: hams_rpc_guard:public_rpc]` `[@ANCHOR: hams_rpc_guard:decorator_marks]`
* In a file: `rpc_exposure.json` in the addon's root. The installed addons' files are merged once per registry.
  `[@ANCHOR: hams_rpc_guard:load_exposure]` `[@ANCHOR: hams_rpc_guard:exposure_init]`
  `[@ANCHOR: hams_rpc_guard:exposure_merge]` `[@ANCHOR: hams_rpc_guard:pairs]` `[@ANCHOR: hams_rpc_guard:manifest_of]`
* The transitional list (`internal`) of every addon of a repository is generated from its source, never by hand:
  `python3 hams_rpc_guard/exposure_scan.py --write .` (check: `--check .`). A test fails when a new public method is on
  neither the list nor the decorator. `[@ANCHOR: hams_rpc_guard:scan_main]` `[@ANCHOR: hams_rpc_guard:scan_addon]`
  `[@ANCHOR: hams_rpc_guard:scan_source]` `[@ANCHOR: hams_rpc_guard:scan_model_of_class]`
  `[@ANCHOR: hams_rpc_guard:scan_orm_names]` `[@ANCHOR: hams_rpc_guard:scan_render]`
  `[@ANCHOR: hams_rpc_guard:scan_managed_addons]` `[@ANCHOR: hams_rpc_guard:scan_decorator_names]` (the names of a
  method's decorators) `[@ANCHOR: hams_rpc_guard:scan_string_values]` (the string or list of strings a class attribute holds)
* A daemon's entry names its source files; a test reads them and requires every model and method the daemon calls to
  be in the entry, so a daemon keeps working when its code changes only together with its allowlist.
  `[@ANCHOR: hams_rpc_guard:scan_daemon_pairs]`

## Verification

`tests/test_policy.py` (the decision table, modes, counters), `tests/test_exposure.py` (decorator, files, scanner),
`tests/test_guard.py` (the wrapper with real users, and through `call_kw` and XML-RPC), `tests/test_repository.py` (the
checked-in lists are current, every daemon pair is allowed).
