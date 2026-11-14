# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Keeps a RealTransactionCase from leaving Cloudflare credentials on the shared website."""

CREDENTIAL_FIELDS = ("cloudflare_api_token_crypt", "cloudflare_zone_id")


def restore_credentials_after_test(case, website):
    """Puts `website`'s Cloudflare credentials back, and commits, when `case` ends.

    The code under test commits (process_queue does), so the write that gives the shared
    website a token and zone outlives the test's rollback. A later test that expects a
    website without credentials (the post-init hook test) then fails, but only when it runs
    after this class. Call it from setUp, after super().setUp(): cleanups run last in, first
    out, so this runs before the harness closes the cursor."""
    original = website.read(list(CREDENTIAL_FIELDS))[0]
    original.pop("id")

    def restore():
        case.env.cr.rollback()
        website.write(original)
        case.env.cr.commit()

    case.addCleanup(restore)
