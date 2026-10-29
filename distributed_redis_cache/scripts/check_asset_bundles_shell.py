# SPDX-License-Identifier: AGPL-3.0-or-later
"""Compile every stylesheet bundle of a database and report whether any fails. Piped into `odoo shell` by odoo_upgrade.py.

    odoo shell -d hams_prod --no-http < check_asset_bundles_shell.py

Why: Odoo compiles a bundle's SCSS as one source. When the compiler raises (a bare `min(22rem, calc(100vw - 16px))`, an
undefined variable), Odoo does not fail anything: it serves HTTP 200 with a stub stylesheet (`css_error_message`, a red
"A css error occured" banner) and the whole site is unstyled. hams.com was unstyled for ~16 minutes after the deploy of
2026-10-14 (hams_com #1699), and earlier on 2026-10-11/12. This runs on the server after `odoo -u` and BEFORE odoo is started, so
the site never serves the stub.

It calls only `AssetsBundle.preprocess_css()`, the step `css()` runs before it decides on the stub, which compiles in memory and
writes no attachment; the shell's cursor is rolled back at the end as well. The last line printed is the verdict, and the caller
treats a missing verdict as a failure (the shell's exit status does not reflect an error raised by a piped script):

    ASSET_BUNDLE_CHECK: OK <n> bundle compiles
    ASSET_BUNDLE_CHECK: FAIL <n> failure(s)     (each preceded by an "ASSET_BUNDLE_CHECK: <bundle> ..." line)

`env` is the shell's environment. When this file is imported or exec'd with a `__name__` other than "__main__" (ham_base's
test_asset_bundles_compile.py does that on a test database) nothing runs until `check(env)` is called.
"""

import logging

logger = logging.getLogger(__name__)

BUNDLES = ("web.assets_frontend", "web.assets_frontend_lazy", "web.assets_web", "web.assets_backend", "website.assets_wysiwyg")
STYLE_SUFFIXES = (".scss", ".sass", ".less", ".css")
PREFIX = "ASSET_BUNDLE_CHECK:"


def check(env):
    """Return (compiles run, failure lines) for every bundle of BUNDLES that has a stylesheet, for the no-website case and
    for each website (a theme module's assets join only its own website's bundle). Debug mode does not change the compile."""
    params = dict(env["ir.asset"]._get_asset_params())
    website_ids = [False] + env["website"].search([], limit=1000).ids if "website" in env else [False]
    runs, failures = 0, []
    for name in BUNDLES:
        paths = env["ir.asset"]._get_asset_paths(name, params)
        if not any(str(path).endswith(STYLE_SUFFIXES) for path, _full, _bundle, _modified in paths):
            continue  # not in this database (the website modules are optional) or it has no stylesheet
        for website_id in website_ids:
            bundle_params = dict(params, website_id=website_id)
            bundle = env["ir.qweb"]._get_asset_bundle(name, css=True, js=False, assets_params=bundle_params)
            bundle.css_errors = []
            bundle.preprocess_css()
            runs += 1
            for error in bundle.css_errors:
                first = next((line.strip() for line in str(error).splitlines() if line.strip()), "")
                failures.append(f"{name} (website {website_id or 'none'}): {first}")
    return runs, failures


def report(env):
    try:
        runs, failures = check(env)
    except Exception as error:  # audit-ignore-catch-all: whatever goes wrong, a failing verdict line must be printed
        logger.exception("the stylesheet bundles could not be compiled")
        print(f"{PREFIX} FAIL could not compile the bundles: {error!r}")
        return
    finally:
        env.cr.rollback()
    for line in failures:
        print(f"{PREFIX} {line}")
    if failures:
        print(f"{PREFIX} FAIL {len(failures)} failure(s)")
    elif runs == 0:
        print(f"{PREFIX} FAIL no stylesheet bundle was found to compile")
    else:
        print(f"{PREFIX} OK {runs} bundle compiles")


if __name__ == "__main__":
    report(globals()["env"])  # `env` is provided by `odoo shell`
