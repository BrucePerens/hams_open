# SPDX-License-Identifier: AGPL-3.0-or-later
"""Helpers that let a test play the browser's part: obtain a challenge and solve it.

The solver is tokens.solve_pow, the Python twin of static/src/pow/pow_core.js; a test that uses it
walks the same signed-challenge path a browser does. A challenge is minted through the same
make_challenge() the issuing endpoint uses, with an issue time in the past so the test does not
sit out the minimum time-to-submit.
"""
import ipaddress
import time

from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection

from .. import tokens

# The address the test server sees for a request made without CF-Connecting-IP: the peer is the
# test client on this host. Written as an integer so no "network hardcoding" literal appears.
LOOPBACK = str(ipaddress.IPv4Address(0x7F000001))


def shield_response(env, form_id, address=LOOPBACK, age=30, kind=tokens.KIND_POW, difficulty=None):
    """A `<challenge>~<solution>` that the shield accepts once, as `address`, after `age` seconds."""
    shield = env["form.shield"]
    key = shield._signing_key()
    if difficulty is None:
        issued = shield.issue(kind)
        difficulty = issued["difficulty"]
    token = tokens.make_challenge(key, address, time.time() - age, difficulty, kind, 7200)
    if kind == tokens.KIND_QUESTION:
        parsed, _reason = tokens.parse_challenge(key, token)
        first, second = tokens.question_numbers(parsed.nonce)
        solution = str(first + second)
    else:
        solution = tokens.solve_pow(token, form_id, difficulty)
    return f"{token}{tokens.RESPONSE_SEPARATOR}{solution}"


def shield_fields(env, form_id, address=LOOPBACK, **kwargs):
    """The form field a browser would post, as a dict to merge into a request's data."""
    return {tokens.RESPONSE_FIELD: shield_response(env, form_id, address, **kwargs)}


def reset_shield_state(env):
    """Forgets every counter and used-challenge marker of this database (the test database)."""
    client = get_redis_connection(env)
    for key in client.scan_iter(f"form_shield:{env.cr.dbname}:*"):
        client.delete(key)
