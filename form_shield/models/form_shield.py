# SPDX-License-Identifier: AGPL-3.0-or-later
"""The form shield: signed proof-of-work challenges, one-use enforcement and rate limits.

A form calls `env["form.shield"].check(...)` once per submission and acts on the verdict. The
cryptographic part is in tokens.py and holds no state. The state is Redis only: a one-use marker
per challenge and a few counters, every key expiring on its own, none containing an address
(only a keyed hash of the visitor's network, which is also what the rate limits count). Redis
being unreachable is reported as the UNAVAILABLE verdict, never skipped: a shield that quietly
switches itself off when its store is down is the failure mode a spammer waits for.
"""
import hashlib
import logging
import time
from collections import namedtuple

import redis

from odoo import api, models

from odoo.addons.distributed_redis_cache.redis_pool import (
    get_redis_connection,
)

from .. import tokens

_logger = logging.getLogger(__name__)

DEFAULT_DIFFICULTY = 19
DEFAULT_TTL = 7200
DEFAULT_MIN_AGE = 3
DEFAULT_QUESTION_MIN_AGE = 6
HIGHEST_DIFFICULTY = 28

OK = "ok"
SILENT = "silent"
RETRY = "retry"
QUESTION = "question"
LIMITED = "limited"
UNAVAILABLE = "unavailable"

Verdict = namedtuple("Verdict", "ok outcome reason")

# Per form: (scope, allowed, window seconds). "subject" is the visitor's network for a guest and
# the user for a member; "target" is whatever the form is about (a page, a listing); "global"
# is the whole form. A form with no entry gets DEFAULT_LIMITS.
DEFAULT_LIMITS = (
    ("subject", 6, 3600),
    ("target", 20, 86400),
    ("global", 300, 3600),
)
MEMBER_LIMITS = (
    ("subject", 20, 3600),
    ("target", 20, 86400),
    ("global", 300, 3600),
)


class FormShield(models.AbstractModel):
    _name = "form.shield"
    _description = "Form shield (anti-spam checks for public forms)"

    # ------------------------------------------------------------------ configuration

    # [@ANCHOR: form_shield:int_param]
    # Verified by [@ANCHOR: form_shield:test_check_refusal_table]
    @api.model
    def _int_param(self, name, default):
        raw = self.env["ir.config_parameter"]._get_param(name)
        try:
            return int(raw) if raw else default
        except ValueError:
            _logger.error("form_shield: %s is not a number: %r", name, raw)
            return default

    # [@ANCHOR: form_shield:signing_key]
    # Verified by [@ANCHOR: form_shield:test_post_init_hook_seeds_key_once]
    @api.model
    def _signing_key(self):
        key = self.env["ir.config_parameter"]._get_param(
            "form_shield.signing_key"
        )
        if not key:
            raise RuntimeError("form_shield.signing_key is not set")
        return key

    # [@ANCHOR: form_shield:client_address]
    # Verified by [@ANCHOR: form_shield:test_challenge_endpoint_issues_signed_uncacheable_json]
    @api.model
    def _client_address(self):
        utils = self.env["zero_sudo.security.utils"]
        return utils._get_trusted_client_ip()

    # ------------------------------------------------------------------ issuing

    # [@ANCHOR: form_shield:issue]
    # Verified by [@ANCHOR: form_shield:test_challenge_endpoint_issues_signed_uncacheable_json]
    @api.model
    def issue(self, kind=tokens.KIND_POW):
        """A new challenge for the current visitor, as the dict the challenge endpoint returns."""
        question = kind == tokens.KIND_QUESTION
        difficulty = 0 if question else self._int_param(
            "form_shield.difficulty", DEFAULT_DIFFICULTY
        )
        difficulty = max(0, min(difficulty, HIGHEST_DIFFICULTY))
        ttl = self._int_param("form_shield.ttl_seconds", DEFAULT_TTL)
        age_param = (
            "form_shield.question_min_age_seconds"
            if question
            else "form_shield.min_age_seconds"
        )
        age_default = DEFAULT_QUESTION_MIN_AGE if question else DEFAULT_MIN_AGE
        min_age = self._int_param(age_param, age_default)
        token = tokens.make_challenge(
            self._signing_key(),
            self._client_address(),
            time.time(),
            difficulty,
            tokens.KIND_QUESTION if question else tokens.KIND_POW,
            ttl,
        )
        issued = {
            "kind": tokens.KIND_QUESTION if question else tokens.KIND_POW,
            "challenge": token,
            "difficulty": difficulty,
            "min_age": min_age,
            "expires_in": ttl,
        }
        if question:
            parsed, _reason = tokens.parse_challenge(
                self._signing_key(), token
            )
            issued["question"] = tokens.question_text(parsed.nonce)
        return issued

    # ------------------------------------------------------------------ checking

    # [@ANCHOR: form_shield:redis_connection]
    # Verified by [@ANCHOR: form_shield:test_check_accepts_a_solved_challenge_once]
    @api.model
    def _redis(self):
        return get_redis_connection(self.env)

    # [@ANCHOR: form_shield:redis_key]
    # Verified by [@ANCHOR: form_shield:test_counters_keep_no_personal_data]
    @api.model
    def _key(self, *parts):
        return ":".join(["form_shield", self.env.cr.dbname, *parts])

    # [@ANCHOR: form_shield:reject]
    # Verified by [@ANCHOR: form_shield:test_check_refusal_table]
    @api.model
    def _reject(self, form_id, reason, outcome):
        _logger.info(
            "form_shield: %s refused (%s, outcome %s)",
            form_id,
            reason,
            outcome,
        )
        own_cursor = bool(self.env.context.get("form_shield_own_cursor"))
        self.env["form.shield.rejection"]._count(form_id, reason, own_cursor)
        return Verdict(False, outcome, reason)

    # [@ANCHOR: form_shield:refuse]
    # Verified by [@ANCHOR: user_websites:test_report_problem_reasons]
    @api.model
    def refuse(self, form_id, reason, outcome=RETRY):
        """Counts a refusal the caller decided on itself (a field a form checks after the
        shield passed, say) under the same per-day counters, and returns the Verdict."""
        return self._reject(form_id, reason, outcome)

    # [@ANCHOR: form_shield:check]
    # Verified by [@ANCHOR: form_shield:test_check_accepts_a_solved_challenge_once]
    # Verified by [@ANCHOR: form_shield:test_check_refusal_table]
    @api.model
    def check(
        self,
        form_id,
        response,
        honeypots=(),
        target=None,
        limits=None,
        own_cursor=False,
    ):
        """Decides whether this submission may proceed. Returns a Verdict.

        `response` is the visitor's `<challenge>~<solution>` (ignored for a signed-in member,
        who is checked by the honeypots and the rate limits only: members' pages are never
        edge-cached and a member is accountable for what they send). `honeypots` are the values
        of fields a person never sees; any text in one marks a program. `target` is whatever the
        form is about, so one page cannot be flooded from many addresses.
        """
        if own_cursor:
            self = self.with_context(form_shield_own_cursor=True)
        if any((value or "").strip() for value in honeypots):
            return self._reject(form_id, "honeypot", SILENT)
        member = not self.env.user._is_public()
        try:
            if not member:
                failure = self._check_response(form_id, response)
                if failure:
                    return failure
            subject = (
                f"user:{self.env.user.id}"
                if member
                else "net:"
                + tokens.ip_tag(
                    self._signing_key(), self._client_address()
                )
            )
            table = limits or (MEMBER_LIMITS if member else DEFAULT_LIMITS)
            exceeded = self._over_limit(form_id, subject, target, table)
            if exceeded:
                return self._reject(form_id, exceeded, LIMITED)
        except (redis.exceptions.RedisError, OSError) as error:
            _logger.error("form_shield: Redis unavailable: %s", error)
            return self._reject(form_id, "unavailable", UNAVAILABLE)
        self.env["form.shield.rejection"]._count(form_id, "accepted")
        return Verdict(True, OK, "accepted")

    # [@ANCHOR: form_shield:check_form]
    # Verified by [@ANCHOR: form_shield:test_check_form_reads_the_standard_fields]
    @api.model
    def check_form(self, form_id, post, honeypots=(), target=None, limits=None):
        """`check()` for a submitted HTML form: reads the standard fields off `post` (a dict of
        the posted values). `shield_response` is the proof of work's answer; a visitor who used
        the question form posts `shield_challenge` and `shield_answer` instead. `fs_homepage`
        is the shield's own honeypot; a form may add its own through `honeypots`."""
        response = (post.get(tokens.RESPONSE_FIELD) or "").strip()
        if not response and post.get(tokens.QUESTION_CHALLENGE_FIELD):
            response = "%s%s%s" % (
                post.get(tokens.QUESTION_CHALLENGE_FIELD),
                tokens.RESPONSE_SEPARATOR,
                post.get(tokens.QUESTION_ANSWER_FIELD) or "",
            )
        traps = [post.get(tokens.HONEYPOT_FIELD), *honeypots]
        return self.check(form_id, response, traps, target, limits)

    # [@ANCHOR: form_shield:check_response]
    # Verified by [@ANCHOR: form_shield:test_check_refusal_table]
    def _check_response(self, form_id, response):
        """None when a guest's challenge answer is good, else the refusing Verdict."""
        key = self._signing_key()
        token, solution = tokens.split_response(response)
        if token is None:
            return self._reject(form_id, "no_token", QUESTION)
        parsed, reason = tokens.parse_challenge(key, token)
        if parsed is None:
            return self._reject(form_id, reason, SILENT)
        now = time.time()
        if now > parsed.expires:
            return self._reject(form_id, tokens.EXPIRED, RETRY)
        if parsed.tag != tokens.ip_tag(key, self._client_address()):
            return self._reject(form_id, tokens.IP_CHANGED, RETRY)
        if parsed.kind == tokens.KIND_QUESTION:
            min_age = self._int_param(
                "form_shield.question_min_age_seconds",
                DEFAULT_QUESTION_MIN_AGE,
            )
        else:
            min_age = self._int_param(
                "form_shield.min_age_seconds", DEFAULT_MIN_AGE
            )
        if now < parsed.issued + min_age:
            return self._reject(form_id, tokens.TOO_FAST, RETRY)
        if parsed.kind == tokens.KIND_QUESTION:
            if not tokens.question_valid(parsed.nonce, solution):
                return self._reject(form_id, "wrong_answer", RETRY)
        elif not tokens.pow_valid(
            token, form_id, solution, parsed.difficulty
        ):
            return self._reject(form_id, tokens.BAD_SOLUTION, SILENT)
        if not self._claim_nonce(parsed, now):
            return self._reject(form_id, "replay", SILENT)
        return None

    # [@ANCHOR: form_shield:claim_nonce]
    # Verified by [@ANCHOR: form_shield:test_check_accepts_a_solved_challenge_once]
    def _claim_nonce(self, parsed, now):
        """True the first time this challenge is presented, False every time after."""
        remaining = max(int(parsed.expires - now), 1) + 60
        claimed = self._redis().set(
            self._key("nonce", parsed.nonce), "1", nx=True, ex=remaining
        )
        return bool(claimed)

    # [@ANCHOR: form_shield:over_limit]
    # Verified by [@ANCHOR: form_shield:test_check_refusal_table]
    def _over_limit(self, form_id, subject, target, table):
        """The scope name of the first exceeded limit, else None. Each call counts one use."""
        client = self._redis()
        values = {
            "subject": subject,
            "target": (
                hashlib.sha256((target or "").encode()).hexdigest()[:24]
                if target
                else None
            ),
            "global": "all",
        }
        exceeded = None
        for scope, allowed, window in table:
            value = values.get(scope)
            if value is None:
                continue
            key = self._key("limit", form_id, scope, value)
            used = client.incr(key)
            if used == 1 or client.ttl(key) < 0:
                client.expire(key, window)
            if used > allowed and exceeded is None:
                exceeded = "rate_" + scope
        return exceeded
