# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of the Tier B broker (secret_broker/broker.py), added to kill the mutation survivors of the second
mutation pass (2026-11-13): exact refusal codes and messages, limits at their exact edges, the ACL parser byte by byte,
the path filter, what `https_call` hands to the HTTPS client, and the connection loop."""
import base64
import errno
import hashlib
import hmac
import json
import os
import shutil
import socket
import stat
import struct
import tempfile
import threading
import time
from types import SimpleNamespace

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.tests.common import SafePatchMixin
from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.daemon.secret_broker import broker as sb

ME = os.getuid()


def _acl(*entries, version=2):
    raw = struct.pack("<I", version)
    for tag, perm, qualifier in entries:
        raw += struct.pack("<HHI", tag, perm, qualifier)
    return raw


def _config(**overrides):
    config = {"socket": "/run/x/b.sock", "allowed_users": ["web"], "deny_readers": ["web"], "credentials": "systemd",
              "secrets": {"key": {"operations": ["hmac_sign"]}}}
    config.update(overrides)
    return config


def _https(**overrides):
    block = {"base_url": "https://api.example.com", "methods": ["GET"], "path_prefixes": ["/v1/"], "auth": "bearer"}
    block.update(overrides)
    return block


def _with_https(**overrides):
    return _config(secrets={"key": {"operations": ["https_request"], "https": _https(**overrides)}})


class _Case(SafePatchMixin, BaseCase):
    def setUp(self):
        super().setUp()
        self.dir = tempfile.mkdtemp(prefix="broker_units_")
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)

    def refused(self, function, *args, code="path_not_allowed"):
        with self.assertRaises(sb.Refused) as caught:
            function(*args)
        self.assertEqual(caught.exception.code, code)
        return caught.exception


@tagged("post_install", "-at_install")
class TestBrokerConfig(_Case):
    # Tests [@ANCHOR: zero_sudo:secret_broker_validate_config]

    def bad(self, config, message):
        with self.assertRaises(sb.BrokerConfigError) as caught:
            sb.validate_config(config)
        self.assertEqual(str(caught.exception), message)

    def test_refused_carries_its_code(self):
        refusal = sb.Refused("why")
        self.assertEqual((refusal.code, str(refusal), refusal.args), ("why", "why", ("why",)))

    def test_top_level_messages(self):
        self.bad([], "the config must be a JSON object")
        self.bad(_config(socket="rel/b.sock"), "'socket' must be an absolute path")
        self.bad(_config(socket=5), "'socket' must be an absolute path")
        for key in ("allowed_users", "deny_readers"):
            for value in (None, "web", [], [None], [["x"]]):
                self.bad(_config(**{key: value}), "'%s' must be a non-empty list of account names or uids" % key)
        self.assertEqual(sb.validate_config(_config(allowed_users=["web", 1000], deny_readers=[0])), _config(allowed_users=["web", 1000], deny_readers=[0]))
        for value in ("relative", None, 5, ""):
            self.bad(_config(credentials=value), "'credentials' must be 'systemd' or an absolute file path")
        self.assertTrue(sb.validate_config(_config(credentials="/etc/x/secrets")))
        for value in (None, {}, []):
            self.bad(_config(secrets=value), "'secrets' must name at least one secret")

    def test_secret_names_and_operations(self):
        with self.assertRaises(ValueError) as caught:
            sb.validate_config(_config(secrets={"Bad Name": {"operations": ["hmac_sign"]}}))
        self.assertEqual(str(caught.exception), "invalid secret name 'Bad Name'")
        message = "secret 'key': 'operations' must be a subset of %s" % (sb.OPERATIONS,)
        for policy in ("text", None, {}, {"operations": []}, {"operations": "hmac_sign"}, {"operations": ["nope"]},
                       {"operations": ["hmac_sign", "nope"]}):
            self.bad(_config(secrets={"key": policy}), message)
        self.assertTrue(sb.validate_config(_config(secrets={"key": {"operations": list(sb.OPERATIONS[:3])}})))

    def test_https_policy_messages_and_edges(self):
        self.bad(_config(secrets={"key": {"operations": ["https_request"]}}), "secret 'key': https_request needs an 'https' block")
        self.bad(_config(secrets={"key": {"operations": ["https_request"], "https": "x"}}), "secret 'key': https_request needs an 'https' block")
        base = "secret 'key': 'base_url' must be https://host[:port] with nothing else"
        for url in ("http://h", "https://", "https://u@h", "https://h/x", "https://h?q=1", "", "h"):
            self.bad(_with_https(base_url=url), base)
        for url in ("https://h", "https://h/", "https://h:8443", "https://h:8443/"):
            self.assertTrue(sb.validate_config(_with_https(base_url=url)))
        methods = "secret 'key': 'methods' must be a subset of %s" % (sorted(sb.HTTP_METHODS),)
        for value in (None, [], "GET", ["GET", "TRACE"], ["get"]):
            self.bad(_with_https(methods=value), methods)
        self.assertTrue(sb.validate_config(_with_https(methods=sorted(sb.HTTP_METHODS))))  # the whole set is allowed
        prefixes = "secret 'key': 'path_prefixes' must be a list of paths starting with /"
        for value in (None, [], "/v1/", ["/v1/", "v2"], [5]):
            self.bad(_with_https(path_prefixes=value), prefixes)
        auth = "secret 'key': 'auth' must be bearer, basic (+username) or header (+header_name)"
        for value in ({"auth": None}, {"auth": "digest"}, {"auth": "basic"}, {"auth": "basic", "username": ""},
                      {"auth": "header"}, {"auth": "header", "header_name": ""}):
            self.bad(_with_https(**value), auth)
        for value in ({"auth": "bearer"}, {"auth": "basic", "username": "u"}, {"auth": "header", "header_name": "X-Key"}):
            self.assertTrue(sb.validate_config(_with_https(**value)))
        timeout = "secret 'key': 'timeout' must be 1 to 120 seconds"
        for value in (0, 0.5, 121, 120.5, "20", None, [1]):
            self.bad(_with_https(timeout=value), timeout)
        for value in (1, 1.5, 20, 120):
            self.assertTrue(sb.validate_config(_with_https(timeout=value)))


@tagged("post_install", "-at_install")
class TestBrokerAcl(_Case):
    # Tests [@ANCHOR: zero_sudo:secret_broker_acl_grants_read]
    # Tests [@ANCHOR: zero_sudo:secret_broker_readable_by]
    # Tests [@ANCHOR: zero_sudo:secret_broker_account_ids]
    # Tests [@ANCHOR: zero_sudo:secret_broker_check_not_readable]

    USER, GROUP, MASK = 0x02, 0x08, 0x10

    def test_malformed_acls_count_as_granting(self):
        for raw in (b"", b"\x02\x00\x00", _acl(version=3), _acl(version=1), _acl((self.USER, 4, 5))[:-1], _acl((self.USER, 4, 5)) + b"\x00"):
            self.assertIs(sb.acl_grants_read(raw, 9, {9}), True, raw)
        self.assertIs(sb.acl_grants_read(_acl(), 9, {9}), False)  # a well-formed ACL with no entries grants nothing

    def test_named_entries_and_the_mask(self):
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, 9)), 9, set()), True)  # no mask entry: everything passes
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, 8)), 9, set()), False)
        self.assertIs(sb.acl_grants_read(_acl((self.GROUP, 4, 70)), 9, {70}), True)
        self.assertIs(sb.acl_grants_read(_acl((self.GROUP, 4, 70)), 9, {71}), False)
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, 70)), 70, set()), True)
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, 9)), 70, {9}), False)  # a user entry is not a group entry
        self.assertIs(sb.acl_grants_read(_acl((self.GROUP, 4, 9)), 9, set()), False)  # and a group entry is not a user entry
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 1, 9)), 9, set()), False)  # execute only
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 2, 9)), 9, set()), False)  # write only
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 6, 9)), 9, set()), True)
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 0x0C, 9)), 9, set()), True)  # only the low three bits count
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 0x08, 9)), 9, set()), False)
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, 9), (self.MASK, 0, 0)), 9, set()), False)
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, 9), (self.MASK, 4, 0)), 9, set()), True)
        self.assertIs(sb.acl_grants_read(_acl((self.MASK, 0, 0), (self.USER, 4, 9)), 9, set()), False)  # the mask may come first
        self.assertIs(sb.acl_grants_read(_acl((self.MASK, 6, 0), (self.USER, 4, 9)), 9, set()), True)

    def test_tag_and_qualifier_use_their_full_width(self):
        self.assertIs(sb.acl_grants_read(_acl((0x0102, 4, 9)), 9, {9}), False)  # tag 0x102 is not USER (0x02)
        self.assertIs(sb.acl_grants_read(_acl((0x0108, 4, 9)), 9, {9}), False)
        big = 0x01000009
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, big)), big, set()), True)
        self.assertIs(sb.acl_grants_read(_acl((self.USER, 4, big)), 9, set()), False)

    def _file(self, mode):
        path = os.path.join(self.dir, "f")
        if os.path.exists(path):
            os.chmod(path, 0o600)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("x")
        os.chmod(path, mode)
        return path

    def test_readable_by_mode_bits(self):
        path = self._file(0o600)
        self.assertIs(sb.readable_by(path, 0, set()), True)  # root
        self.assertIs(sb.readable_by(path, ME, set()), True)
        os.chmod(path, 0o200)
        self.assertIs(sb.readable_by(path, ME, {os.getgid()}), False)  # the owner's own bits decide, not group or other
        os.chmod(path, 0o040)
        self.assertIs(sb.readable_by(path, ME + 1, {os.getgid()}, getxattr=None), True)
        self.assertIs(sb.readable_by(path, ME + 1, {os.getgid() + 1}, getxattr=None), False)
        os.chmod(path, 0o020)
        self.assertIs(sb.readable_by(path, ME + 1, {os.getgid()}, getxattr=None), False)
        os.chmod(path, 0o004)
        self.assertIs(sb.readable_by(path, ME + 1, set(), getxattr=None), True)
        os.chmod(path, 0o002)
        self.assertIs(sb.readable_by(path, ME + 1, set(), getxattr=None), False)
        os.chmod(path, 0o000)
        self.assertIs(sb.readable_by(path, ME + 1, set(), getxattr=None), False)

    def test_readable_by_acl_lookup(self):
        path = self._file(0o600)
        asked = []

        def returning(raw):
            return lambda p, name: asked.append((p, name)) or raw

        def failing(code):
            def getxattr(p, name):
                raise OSError(code, "x")
            return getxattr
        self.assertIs(sb.readable_by(path, ME + 1, {5}, getxattr=returning(_acl((0x02, 4, ME + 1)))), True)
        self.assertEqual(asked, [(path, "system.posix_acl_access")])
        self.assertIs(sb.readable_by(path, ME + 1, {5}, getxattr=returning(_acl((0x02, 4, 12345)))), False)
        self.assertIs(sb.readable_by(path, ME + 1, {5}, getxattr=returning(b"junk")), True)
        for code in (errno.ENODATA, errno.ENOTSUP):
            self.assertIs(sb.readable_by(path, ME + 1, {5}, getxattr=failing(code)), False)
        with self.assertLogs("hams_secret_broker", level="WARNING") as logs:
            self.assertIs(sb.readable_by(path, ME + 1, {5}, getxattr=failing(errno.EIO)), True)
        self.assertEqual(logs.output, ["WARNING:hams_secret_broker:cannot read the ACL of %s (errno %d); treating it as readable" % (path, errno.EIO)])
        self.assertIs(sb.readable_by.__defaults__[0], os.getxattr)

    def test_account_ids(self):
        entry = SimpleNamespace(pw_name="bob", pw_gid=5, pw_uid=9)
        self.safe_patch_object(sb.pwd, "getpwnam", new=lambda name: entry)
        self.safe_patch_object(sb.pwd, "getpwuid", new=lambda uid: SimpleNamespace(pw_name="bob", pw_gid=6, pw_uid=uid))
        self.safe_patch_object(sb.grp, "getgrall", new=lambda: [SimpleNamespace(gr_gid=77, gr_mem=["bob", "x"]),
                                                                SimpleNamespace(gr_gid=88, gr_mem=["alice"])])
        self.assertEqual(sb.account_ids("bob"), (9, {5, 77}))
        self.assertEqual(sb.account_ids(1234), (1234, {6, 77}))
        self.assertEqual(sb.account_ids(0)[0], 1234 * 0 + 0)

    def test_account_ids_real_root(self):
        self.assertEqual(sb.account_ids("root")[0], 0)
        self.assertIn(0, sb.account_ids(0)[1])

    def test_check_not_readable(self):
        path = self._file(0o644)
        with self.assertRaises(sb.BrokerConfigError) as caught:
            sb.check_not_readable([path], ["web"], ids=lambda user: (ME + 1, {5}))
        self.assertEqual(str(caught.exception), "%s is readable by 'web'; the web account must not read broker secrets" % path)
        os.chmod(path, 0o600)
        sb.check_not_readable([path], ["web"], ids=lambda user: (ME + 1, {5}))
        seen = []
        sb.check_not_readable([path, path], ["a", "b"], ids=lambda user: seen.append(user) or (ME + 1, {5}))
        self.assertEqual(seen, ["a", "b"])


@tagged("post_install", "-at_install")
class TestRedactScrubPath(_Case):
    # Tests [@ANCHOR: zero_sudo:secret_broker_redact]
    # Tests [@ANCHOR: zero_sudo:secret_broker_scrub]
    # Tests [@ANCHOR: zero_sudo:secret_broker_check_path]

    def test_redact(self):
        self.assertEqual(sb.redact("a abcd b", "abcd"), "a [redacted by secret broker] b")
        self.assertEqual(sb.redact("a abc b", "abc"), "a abc b")  # under four characters is not searched
        self.assertEqual(sb.redact("x", ""), "x")
        self.assertEqual(sb.redact("a k1k1 b ZZZZ", "k1k1", ["ZZZZ", "", "yy"]), "a [redacted by secret broker] b [redacted by secret broker]")

    def test_scrub_reaches_into_lists_dicts_and_keys(self):
        secret = "sekret"
        value = {"k_sekret": ["a sekret", {"n": 1, "s": "sekret"}, 2.5, None, True], "plain": "ok"}
        out = sb.scrub(value, secret)
        red = sb.REDACTED
        self.assertEqual(out, {"k_" + red: ["a " + red, {"n": 1, "s": red}, 2.5, None, True], "plain": "ok"})
        self.assertIsInstance(out["k_" + red], list)
        self.assertEqual(sb.scrub(7, secret), 7)

    def test_paths_that_are_allowed(self):
        for path in ("/v1/charges", "/v1/charges?a=1&b=http://x//y", "/v1/a!b", "/v1/a~b", "/v1/a%41b", "/v1/%252541", "/v1/x?u=//", "/v1", "/v1/aXb"):
            self.assertEqual(sb.check_path(path, ["/v1/"]), path)
        self.assertEqual(sb.check_path("/v1/x", ["/v1"]), "/v1/x")
        self.assertEqual(sb.check_path("/vX/y", ["/vX/"]), "/vX/y")
        self.assertEqual(sb.check_path("/vX/y", ["/vX"]), "/vX/y")
        self.assertEqual(sb.check_path("/v1", ["/v1/", "/other"]), "/v1")
        self.assertEqual(sb.check_path("/other/z", ["/v1/", "/other"]), "/other/z")

    def test_paths_that_are_refused(self):
        for path in (5, None, "v1/x", "", "//v1/x", "/v1//x", "/v1/a/../b", "/v1/..", "/v1/%2e%2e/x", "/v1/a%2eb", "/v1/a%2Fb",
                     "/v1/a#frag", "/v1/a@b", "/v1/a\\b", "/v1/a b", "/v1/a\tb", "/v1/a\nb", "/v1/a\x7fb", "/v1/a\x00b",
                     "/v1/a%5cb", "/v1/a%00b", "/v1/%252e%252e/x", "/v1/%25252e%25252e/x", "/v1/a%252fb", "/v1/%25252541",
                     "/v1/%2525252541", "/v1x/y", "/v2/x", "/", "/v1?x"[:0] + "/x"):
            self.refused(sb.check_path, path, ["/v1/"])
        self.refused(sb.check_path, "/v1x", ["/v1"])
        self.refused(sb.check_path, "/v1x/y", ["/v1"])
        self.refused(sb.check_path, "/v1/x", [])


class _Response:
    def __init__(self, data, status=200, headers=None):
        self.data, self.status, self.headers, self.reads = data, status, headers or {}, []

    def read(self, amount):
        self.reads.append(amount)
        return self.data[:amount]

    def getheader(self, name, default=""):
        self.headers_asked.append((name, default))
        return self.headers.get(name, default)

    headers_asked = None


class _FakeConnection:
    instances = []

    def __init__(self, response, host, port, timeout, context, fail=False):
        self.response, self.args, self.fail, self.closed, self.requests = response, (host, port, timeout, context), fail, 0, []
        self.closed = False

    def request(self, method, path, body=None, headers=None):
        self.requests.append((method, path, body, headers))
        if self.fail:
            raise OSError("boom")

    def getresponse(self):
        return self.response

    def close(self):
        self.closed = True


@tagged("post_install", "-at_install")
class TestHttpsCall(_Case):
    # Tests [@ANCHOR: zero_sudo:secret_broker_https_call]

    POLICY = {"base_url": "https://api.example.com", "methods": ["GET", "POST"], "path_prefixes": ["/v1/"], "auth": "bearer"}

    def _call(self, policy=None, secret="sk_secret_1", method="GET", path="/v1/x", body=None, headers=None, data=b"{}", fail=False,
              status=200, response_headers=None):
        policy = dict(self.POLICY, **(policy or {}))
        response = _Response(data, status, response_headers)
        response.headers_asked = []
        made = []
        contexts = []

        def connection(host, port, timeout=None, context=None):
            made.append(_FakeConnection(response, host, port, timeout, context, fail))
            return made[-1]
        self.safe_patch_object(sb.http.client, "HTTPSConnection", new=connection)
        self.safe_patch_object(sb.ssl, "create_default_context", new=lambda cafile=None: contexts.append(cafile) or "CTX")
        self.response, self.made, self.contexts = response, made, contexts
        return sb.https_call(policy, secret, method, path, body, headers)

    def test_result_shape_and_what_is_sent(self):
        result = self._call(data=b'{"a": "x"}', status=201, response_headers={"Content-Type": "application/json", "Location": "https://e/?k=sk_secret_1"})
        self.assertEqual(result, {"status": 201, "content_type": "application/json", "location": "https://e/?k=" + sb.REDACTED, "body": '{"a": "x"}'})
        connection = self.made[0]
        self.assertEqual(connection.args, ("api.example.com", 443, 20, "CTX"))
        self.assertEqual(connection.requests, [("GET", "/v1/x", None, {"Authorization": "Bearer sk_secret_1"})])
        self.assertTrue(connection.closed)
        self.assertEqual(self.response.headers_asked, [("Content-Type", ""), ("Location", "")])
        self.assertEqual(self.response.reads, [sb.MAX_RESPONSE_BYTES + 1])
        self.assertEqual(self.contexts, [None])

    def test_missing_headers_come_back_empty(self):
        result = self._call()
        self.assertEqual((result["content_type"], result["location"]), ("", ""))

    def test_port_timeout_and_ca_file_come_from_the_policy(self):
        self._call(policy={"base_url": "https://h.example:8443", "timeout": 7, "ca_file": "/ca.pem"})
        self.assertEqual(self.made[0].args, ("h.example", 8443, 7, "CTX"))
        self.assertEqual(self.contexts, ["/ca.pem"])

    def test_auth_schemes(self):
        self._call(policy={"auth": "basic", "username": "user"}, data=b"echo " + base64.b64encode(b"user:sk_secret_1"))
        pair = base64.b64encode(b"user:sk_secret_1").decode()
        self.assertEqual(self.made[0].requests[0][3], {"Authorization": "Basic " + pair})
        result = self._call(policy={"auth": "basic", "username": "user"}, data=("echo " + pair).encode())
        self.assertEqual(result["body"], "echo " + sb.REDACTED)  # the encoded pair is redacted too
        self._call(policy={"auth": "header", "header_name": "X-Api-Key"}, headers={"Accept": "a/b"})
        self.assertEqual(self.made[0].requests[0][3], {"Accept": "a/b", "X-Api-Key": "sk_secret_1"})

    def test_body_is_sent_as_utf8_bytes_and_empty_is_not_none(self):
        self._call(method="POST", body="hé")
        self.assertEqual(self.made[0].requests[0][2], "hé".encode("utf-8"))
        self._call(method="POST", body="")
        self.assertEqual(self.made[0].requests[0][2], b"")
        self._call(method="POST", body=None)
        self.assertIsNone(self.made[0].requests[0][2])

    def test_the_connection_is_closed_after_a_failure(self):
        with self.assertRaises(OSError):
            self._call(fail=True)
        self.assertTrue(self.made[0].closed)

    def test_response_size_limit_is_exact_and_bad_bytes_are_replaced(self):
        result = self._call(data=b"x" * sb.MAX_RESPONSE_BYTES)
        self.assertEqual(len(result["body"]), sb.MAX_RESPONSE_BYTES)
        with self.assertRaises(sb.Refused) as caught:
            self._call(data=b"x" * (sb.MAX_RESPONSE_BYTES + 1))
        self.assertEqual(caught.exception.code, "response_too_large")
        self.assertEqual(self._call(data=b"a\xffb")["body"], "a�b")

    def test_refusals(self):
        for kwargs, code in (({"method": "DELETE"}, "method_not_allowed"),
                             ({"path": "/other"}, "path_not_allowed"),
                             ({"headers": {"X-Evil": "1"}}, "header_not_allowed"),
                             ({"headers": {5: "1"}}, "header_not_allowed"),
                             ({"headers": {"accept": 5}}, "header_not_allowed"),
                             ({"headers": {"accept": "a\nb"}}, "header_not_allowed"),
                             ({"headers": {"accept": "a\rb"}}, "header_not_allowed"),
                             ({"method": "POST", "body": 5}, "body_not_string"),
                             ({"method": "POST", "body": b"x"}, "body_not_string")):
            with self.assertRaises(sb.Refused) as caught:
                self._call(**kwargs)
            self.assertEqual(caught.exception.code, code, kwargs)
        self.assertEqual(self._call(headers={"Idempotency-Key": "k", "STRIPE-VERSION": "v", "content-type": "t"})["status"], 200)


class _Backend:
    label = "fake"

    def __init__(self, values):
        self.values = values

    def get(self, name):
        return self.values.get(name)


@tagged("post_install", "-at_install")
class TestBrokerHandle(_Case):
    # Tests [@ANCHOR: zero_sudo:secret_broker_broker]
    # Tests [@ANCHOR: zero_sudo:secret_broker_handle]

    def broker(self, secrets=None, **values):
        config = _config(secrets=secrets or {"key": {"operations": list(sb.OPERATIONS[:3])}})
        return sb.Broker(config, ss.SecretStore([_Backend(values or {"key": "sekret-value"})]), {ME})

    def ask(self, broker, op, **args):
        return broker.handle(json.dumps({"v": 1, "op": op, "args": args}).encode(), ME)

    def test_secrets_must_be_present_and_at_least_eight_characters(self):
        sb.Broker(_config(), ss.SecretStore([_Backend({"key": "12345678"})]), {1}).check_secrets_present()
        with self.assertRaises(sb.BrokerConfigError) as caught:
            sb.Broker(_config(), ss.SecretStore([_Backend({"key": "1234567"})]), {1}).check_secrets_present()
        self.assertEqual(str(caught.exception), "secret 'key' is shorter than 8 characters")
        with self.assertRaises(ss.SecretNotFound):
            sb.Broker(_config(), ss.SecretStore([_Backend({})]), {1}).check_secrets_present()

    def test_construction_validates_and_freezes_the_allowed_uids(self):
        with self.assertRaises(sb.BrokerConfigError):
            sb.Broker({}, ss.SecretStore(), {1})
        broker = sb.Broker(_config(), ss.SecretStore(), [3, 4])
        self.assertEqual(broker.allowed_uids, frozenset({3, 4}))

    def test_digest_and_signature_arguments_reach_the_operations(self):
        broker = self.broker()
        expected = hmac.new(b"sekret-value", b"p", hashlib.sha512).hexdigest()
        self.assertEqual(self.ask(broker, "hmac_sign", name="key", payload="p", digest="sha512"), {"ok": True, "result": expected})
        self.assertEqual(self.ask(broker, "hmac_verify", name="key", payload="p", signature=expected, digest="sha512"), {"ok": True, "result": True})
        self.assertEqual(self.ask(broker, "hmac_verify", name="key", payload="p", signature=expected), {"ok": True, "result": False})  # sha256 by default
        sha256 = hmac.new(b"sekret-value", b"p", hashlib.sha256).hexdigest()
        self.assertEqual(self.ask(broker, "hmac_verify", name="key", payload="p", signature=sha256), {"ok": True, "result": True})
        self.assertEqual(self.ask(broker, "hmac_sign", name="key", payload="p", digest="md5")["error"], "bad_request")
        self.assertEqual(self.ask(broker, "hmac_sign", name="key", payload="p", extra=1)["error"], "bad_request")

    def test_stripe_uses_the_default_tolerance_of_300_seconds(self):
        seen = []
        self.safe_patch_object(sb.secret_operations, "stripe_webhook_verify", new=lambda *args: seen.append(args) or True)
        broker = self.broker()
        self.assertEqual(self.ask(broker, "stripe_webhook_verify", name="key", payload="p", signature_header="h"), {"ok": True, "result": True})
        self.assertEqual(seen, [("sekret-value", "p", "h", 300)])
        del seen[:]
        self.ask(broker, "stripe_webhook_verify", name="key", payload="p", signature_header="h", tolerance=5)
        self.assertEqual(seen[0][3], 5)
        self.assertEqual(self.ask(broker, "stripe_webhook_verify", name="key", payload="p", signature_header="h", now=1)["error"], "bad_request")

    def test_an_odd_secret_name_is_an_unknown_secret(self):
        self.assertEqual(self.ask(self.broker(), "hmac_sign", name=["key"], payload="p"), {"ok": False, "error": "unknown_secret"})
        self.assertEqual(self.ask(self.broker(), "hmac_sign", name=5, payload="p"), {"ok": False, "error": "unknown_secret"})
        self.assertEqual(self.ask(self.broker(), "hmac_sign", payload="p"), {"ok": False, "error": "unknown_secret"})


class _TapConnection:
    """A real socket with `settimeout` recorded."""

    def __init__(self, sock):
        self.sock, self.timeouts = sock, []

    def settimeout(self, value):
        self.timeouts.append(value)
        self.sock.settimeout(value)

    def __getattr__(self, name):
        return getattr(self.sock, name)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.sock.close()


@tagged("post_install", "-at_install")
class TestBrokerConnections(_Case):
    # Tests [@ANCHOR: zero_sudo:secret_broker_serve_connection]
    # Tests [@ANCHOR: zero_sudo:secret_broker_serve]
    # Tests [@ANCHOR: zero_sudo:secret_broker_peer_credentials]

    def broker(self, allowed=None):
        config = _config(socket=os.path.join(self.dir, "b.sock"), secrets={"key": {"operations": ["hmac_sign"]}})
        return sb.Broker(config, ss.SecretStore([_Backend({"key": "sekret-value"})]), {ME} if allowed is None else allowed)

    def request(self, payload="p", pad=0):
        body = {"v": 1, "op": "hmac_sign", "args": {"name": "key", "payload": payload + "x" * pad}}
        return json.dumps(body).encode() + b"\n"

    def _serve_one(self, broker, send, then=None, timeout=5):
        """Run serve_connection on one end of a socket pair; `send(client)` writes; returns (reply bytes, tap, thread)."""
        server_end, client = socket.socketpair()
        tap = _TapConnection(server_end)
        worker = threading.Thread(target=broker.serve_connection, args=(tap,), daemon=True)  # burn-ignore-test-daemon-thread: joined below
        worker.start()
        client.settimeout(timeout)
        send(client)
        chunks = []
        while True:
            chunk = client.recv(65536)
            if not chunk:
                break
            chunks.append(chunk)
        if then:
            then(client)
        client.close()
        worker.join(timeout)
        return b"".join(chunks), tap, worker

    def test_peer_credentials_are_this_process(self):
        a, b = socket.socketpair()
        self.addCleanup(a.close)
        self.addCleanup(b.close)
        self.assertEqual(sb.peer_credentials(a), (os.getpid(), ME, os.getgid()))

    def test_an_allowed_peer_gets_an_answer_and_the_log_names_the_outcome(self):
        with self.assertLogs("hams_secret_broker", level="INFO") as logs:
            reply, tap, worker = self._serve_one(self.broker(), lambda c: c.sendall(self.request()))
        self.assertEqual(json.loads(reply)["ok"], True)
        self.assertTrue(reply.endswith(b"\n"))
        self.assertEqual(tap.timeouts, [30])
        self.assertEqual(logs.output, ["INFO:hams_secret_broker:peer uid %d: ok" % ME])
        self.assertFalse(worker.is_alive())

    def test_a_refused_request_logs_its_code_and_a_stranger_is_refused_unread(self):
        with self.assertLogs("hams_secret_broker", level="INFO") as logs:
            reply, tap, worker = self._serve_one(self.broker(), lambda c: c.sendall(b"not json\n"))
        self.assertEqual(json.loads(reply), {"ok": False, "error": "bad_request"})
        self.assertEqual(logs.output, ["INFO:hams_secret_broker:peer uid %d: bad_request" % ME])
        with self.assertLogs("hams_secret_broker", level="INFO") as logs:
            reply, tap, worker = self._serve_one(self.broker(allowed={ME + 1}), lambda c: c.sendall(self.request()))
        self.assertEqual(json.loads(reply), {"ok": False, "error": "peer_not_allowed"})
        self.assertEqual(logs.output, ["INFO:hams_secret_broker:peer uid %d: peer_not_allowed" % ME])

    def test_the_request_is_read_across_several_chunks(self):
        raw = self.request()

        def send(client):
            client.sendall(raw[:10])
            time.sleep(0.05)
            client.sendall(raw[10:])
        reply, tap, worker = self._serve_one(self.broker(), send)
        self.assertEqual(json.loads(reply)["ok"], True)

    def test_a_request_of_exactly_the_limit_is_served_and_one_more_byte_is_too_large(self):
        base = len(self.request())
        exact = self.request(pad=sb.MAX_REQUEST_BYTES - base)
        self.assertEqual(len(exact), sb.MAX_REQUEST_BYTES)
        reply, tap, worker = self._serve_one(self.broker(), lambda c: c.sendall(exact), timeout=20)
        self.assertEqual(json.loads(reply)["ok"], True)
        over = self.request(pad=sb.MAX_REQUEST_BYTES - base + 1)

        def send(client):
            client.sendall(over)
        reply, tap, worker = self._serve_one(self.broker(), send, timeout=20)
        self.assertEqual(json.loads(reply), {"ok": False, "error": "request_too_large"})

    def test_a_peer_that_never_ends_its_request_cannot_hold_the_connection_after_close(self):
        reply, tap, worker = self._serve_one(self.broker(), lambda c: (c.sendall(b'{"v": 1'), c.shutdown(socket.SHUT_WR)))
        self.assertEqual(json.loads(reply), {"ok": False, "error": "bad_request"})
        self.assertFalse(worker.is_alive())

    def test_the_server_ends_its_side_and_stops_listening_to_a_peer_that_keeps_sending(self):
        garbage = b"x" * 65536

        def send(client):
            client.sendall(self.request())

        def then(client):
            for _ in range(sb.MAX_REQUEST_BYTES // 65536 + 3):
                try:
                    client.sendall(garbage)
                except OSError:
                    break
        reply, tap, worker = self._serve_one(self.broker(), send, then=then)
        self.assertEqual(json.loads(reply)["ok"], True)
        self.assertFalse(worker.is_alive())

    def _serving(self, broker, stop_event):
        worker = threading.Thread(target=broker.serve, args=(stop_event,), daemon=True)  # burn-ignore-test-daemon-thread: stopped through stop_event below
        worker.start()
        path = broker.config["socket"]
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if os.path.exists(path) and stat.S_ISSOCK(os.stat(path).st_mode):
                break
            time.sleep(0.01)
        return worker, path

    def _exchange(self, path, raw):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(5)
            client.connect(path)
            client.sendall(raw)
            client.shutdown(socket.SHUT_WR)
            data = b""
            while True:
                chunk = client.recv(65536)
                if not chunk:
                    return data
                data += chunk

    def test_serve_replaces_a_stale_socket_file_sets_its_mode_and_stops_on_the_event(self):
        broker = self.broker()
        path = broker.config["socket"]
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("stale")
        umasks, chmods = [], []
        real_umask, real_chmod = os.umask, os.chmod
        self.safe_patch_object(sb.os, "umask", new=lambda mask: umasks.append(mask) or real_umask(mask))
        self.safe_patch_object(sb.os, "chmod", new=lambda p, mode: chmods.append((p, mode)) or real_chmod(p, mode))
        stop = threading.Event()
        worker, path = self._serving(broker, stop)
        self.assertEqual(json.loads(self._exchange(path, self.request()))["ok"], True)
        self.assertEqual(oct(os.stat(path).st_mode & 0o777), "0o660")
        self.assertEqual(chmods, [(path, 0o660)])
        stop.set()
        worker.join(5)
        self.assertFalse(worker.is_alive())  # the 0.5 s accept timeout lets it notice
        self.assertEqual(umasks[0], 0o117)
        self.assertEqual(len(umasks), 2)  # the old umask is put back

    def test_many_connections_in_a_row_and_a_quiet_spell_do_not_use_up_the_workers(self):
        self.safe_patch_object(sb, "MAX_CONCURRENT", new=2)
        broker = self.broker()
        stop = threading.Event()
        worker, path = self._serving(broker, stop)
        time.sleep(2.2)  # more idle accept timeouts than workers: each must give its slot back
        for _ in range(6):
            self.assertEqual(json.loads(self._exchange(path, self.request()))["ok"], True)
        stop.set()
        worker.join(5)
        self.assertFalse(worker.is_alive())

    def test_a_connection_that_ends_early_is_logged_by_class_name_and_names_its_thread(self):
        names = []

        def broken(conn):
            names.append(threading.current_thread().name)
            raise ConnectionResetError("gone")
        broker = self.broker()
        self.safe_patch_object(broker, "serve_connection", new=broken, create=True)
        stop = threading.Event()
        with self.assertLogs("hams_secret_broker", level="WARNING") as logs:
            worker, path = self._serving(broker, stop)
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(5)
                client.connect(path)
                deadline = time.monotonic() + 5
                while not names and time.monotonic() < deadline:
                    time.sleep(0.01)
            time.sleep(0.2)
            stop.set()
            worker.join(5)
        self.assertEqual(logs.output, ["WARNING:hams_secret_broker:connection ended early: ConnectionResetError"])
        self.assertRegex(names[0], r"^secret-broker_\d+$")
