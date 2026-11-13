# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of `hams_rpc_guard.exposure_scan`, added to kill the mutation survivors of the second mutation
pass (2026-11-13). The scanner only parses source text, so every case is a literal source string or a temporary tree."""
import ast
import contextlib
import io
import json
import os
import tempfile

from odoo.tests.common import BaseCase, tagged
from odoo.addons.zero_sudo.tests.common import SafePatchMixin

from odoo.addons.hams_rpc_guard import exposure_scan as scan


def _scan(source):
    return scan.scan_source(source, scan.orm_names())


@tagged("post_install", "-at_install")
class TestExposureScanUnits(SafePatchMixin, BaseCase):
    # Tests [@ANCHOR: hams_rpc_guard:scan_decorator_names]
    # Tests [@ANCHOR: hams_rpc_guard:scan_string_values]
    # Tests [@ANCHOR: hams_rpc_guard:scan_model_of_class]
    # Tests [@ANCHOR: hams_rpc_guard:scan_source]
    # Tests [@ANCHOR: hams_rpc_guard:scan_addon]
    # Tests [@ANCHOR: hams_rpc_guard:scan_render]
    # Tests [@ANCHOR: hams_rpc_guard:scan_managed_addons]
    # Tests [@ANCHOR: hams_rpc_guard:scan_caller_reference]
    # Tests [@ANCHOR: hams_rpc_guard:scan_module_string_constants]
    # Tests [@ANCHOR: hams_rpc_guard:scan_daemon_pairs]
    # Tests [@ANCHOR: hams_rpc_guard:scan_main]

    def test_decorator_names_of_every_shape(self):
        function = ast.parse("@a.b\n@c\n@d.e(1)\n@f(2)\n@g[0]\n@(h or i)\ndef x(): pass").body[0]
        self.assertEqual(scan._decorator_names(function), {"b", "c", "e", "f"})

    def test_string_values(self):
        def value(text):
            return ast.parse(text, mode="eval").body

        self.assertEqual(scan._string_values(value("'a.b'")), ["a.b"])
        self.assertEqual(scan._string_values(value("['a.b', 5, 'c.d', x]")), ["a.b", "c.d"])
        self.assertEqual(scan._string_values(value("('a.b',)")), ["a.b"])
        self.assertEqual(scan._string_values(value("5")), [])
        self.assertEqual(scan._string_values(value("x")), [])

    def test_model_of_class(self):
        def model(body):
            return scan.model_of_class(ast.parse("class C:\n" + body).body[0])

        self.assertEqual(model("    _name = 'a.b'\n    _inherit = 'c.d'"), "a.b")
        self.assertEqual(model("    _inherit = ['c.d', 'e.f']"), "c.d")
        self.assertEqual(model("    _inherit = 'c.d'\n    _name = 'a.b'"), "a.b")
        self.assertIsNone(model("    other = 'a.b'"))
        self.assertIsNone(model("    _description = 'a.b'\n    x = ['c.d']"))
        self.assertIsNone(model("    _name = 5"))
        self.assertIsNone(model("    '''doc'''\n    def f(self): pass\n    x: int = 1\n    a = b = 'c.d'"))
        self.assertEqual(model("    '''doc'''\n    def f(self): pass\n    _inherit = 'c.d'"), "c.d")
        self.assertEqual(model("    _name, _x = 'a.b', 1\n    _inherit = 'z.z'"), "z.z")

    def test_scan_source_rules(self):
        found = _scan(
            "class A:\n    def public(self): pass\n"  # no _name/_inherit: not a model
            "class B:\n    _name = 'b.b'\n"
            "    def keep(self): pass\n"
            "    def _private(self): pass\n"
            "    @staticmethod\n    def s(): pass\n"
            "    @classmethod\n    def c(cls): pass\n"
            "    @api.private\n    def p(self): pass\n"
            "    @property\n    def prop(self): pass\n"
            "    @cached_property\n    def cached(self): pass\n"
            "    @functools.cached_property\n    def cached2(self): pass\n"
            "    def write(self, v): pass\n"
            "    @public_rpc()\n    def marked(self): pass\n"
            "    @decorators.public_rpc(user='x')\n    def marked2(self): pass\n"
            "    async def waits(self): pass\n"
            "class C(B):\n    _inherit = ['c.c']\n    def extra(self): pass\n    x = 1\n")
        self.assertEqual(sorted(found), ["b.b", "c.c"])
        self.assertEqual(found["b.b"]["public"], {"keep", "marked", "marked2", "waits"})
        self.assertEqual(found["b.b"]["marked"], {"marked", "marked2"})
        self.assertEqual(found["c.c"], {"public": {"extra"}, "marked": set()})
        self.assertEqual(_scan("class B(:\n"), {})
        self.assertEqual(_scan("class B:\n    _name = 'b.b'\n    x = 1\n"), {})
        # nested classes are found too
        self.assertIn("n.n", _scan("def f():\n    class N:\n        _name = 'n.n'\n        def go(self): pass\n"))

    def _tree(self, files):
        root = tempfile.mkdtemp()
        self.addCleanup(lambda: __import__("shutil").rmtree(root, ignore_errors=True))
        for path, text in files.items():
            full = os.path.join(root, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w", encoding="utf-8") as handle:
                handle.write(text)
        return root

    MODEL = "class M:\n    _name = 'x.y'\n    def {name}(self): pass\n"

    def test_scan_addon_without_a_standard_set_and_skips(self):
        files = {"m/models/a.py": self.MODEL.format(name="go"),
                 "m/models/test_x.py": self.MODEL.format(name="from_test"),
                 "m/models/b.txt": self.MODEL.format(name="not_python"),
                 "m/models/.hidden/h.py": self.MODEL.format(name="hidden"),
                 "m/.git/g.py": self.MODEL.format(name="git"),
                 "m/models/marked.py": "class M:\n    _name = 'x.y'\n    @public_rpc\n    def declared(self): pass\n"}
        for skipped in ("tests", "test", "tools", "scripts", "migrations", "static", "daemon", "daemons", "node_modules", "__pycache__", "docs"):
            files["m/%s/s.py" % skipped] = self.MODEL.format(name="skipped_" + skipped)
        root = self._tree(files)
        self.assertEqual(scan.scan_addon(os.path.join(root, "m")), {"x.y": ["go"]})
        self.assertEqual(scan.scan_addon(os.path.join(root, "m"), {"go"}), {})

    def test_scan_addon_merges_and_sorts(self):
        root = self._tree({"m/b.py": "class M:\n    _name = 'z.z'\n    def two(self): pass\n    def one(self): pass\n",
                           "m/a.py": "class M:\n    _name = 'a.a'\n    def three(self): pass\n",
                           "m/c.py": "class M:\n    _inherit = 'z.z'\n    def zero(self): pass\n"})
        self.assertEqual(scan.scan_addon(os.path.join(root, "m")), {"a.a": ["three"], "z.z": ["one", "two", "zero"]})

    def test_render(self):
        text = scan.render({"a.a": ["x"]})
        self.assertTrue(text.endswith("}\n"))
        document = json.loads(text)
        self.assertEqual(list(document), ["comment", "internal"])
        self.assertIn("Transitional allowlist generated by hams_rpc_guard/exposure_scan.py", document["comment"])
        self.assertEqual(text, json.dumps(document, indent=2) + "\n")
        existing = {"comment": "mine", "internal": {"old": ["m"]}, "clients": {"l": []}, "stock": [["a", "b"]],
                    "portal": [["c", "d"]], "junk": 1}
        document = json.loads(scan.render({"n": ["m"]}, existing))
        self.assertEqual(list(document), ["comment", "internal", "portal", "stock", "clients"])
        self.assertEqual(document["comment"], "mine")
        self.assertEqual(document["internal"], {"n": ["m"]})
        self.assertEqual(document["portal"], [["c", "d"]])
        self.assertEqual(document["stock"], [["a", "b"]])
        self.assertEqual(document["clients"], {"l": []})
        self.assertNotIn("junk", document)
        self.assertEqual(json.loads(scan.render({}, {"internal": {"x": ["y"]}}))["internal"], {})
        self.assertIn("comment", json.loads(scan.render({}, {})))

    def test_managed_addons(self):
        root = self._tree({"b/__manifest__.py": "{}", "a/__manifest__.py": "{}", "c/other.py": "", "d/__manifest__.py/x": "", "f.txt": ""})
        self.assertEqual(scan.managed_addons(root), ["a", "b"])

    def test_is_caller_reference(self):
        def node(text):
            return ast.parse(text, mode="eval").body

        self.assertTrue(scan._is_caller_reference(node("client.execute")))
        self.assertTrue(scan._is_caller_reference(node("call_kw")))
        self.assertFalse(scan._is_caller_reference(node("client.other")))
        self.assertFalse(scan._is_caller_reference(node("other")))
        self.assertFalse(scan._is_caller_reference(node("'execute'")))
        self.assertFalse(scan._is_caller_reference(node("f()")))

    def test_module_string_constants(self):
        tree = ast.parse("A = 'a.b'\nB: str = 'm'\nC = 5\nD: int = 6\nE: str\nF, G = 'x', 'y'\nH = I = 'z'\nJ = f('k')\n"
                         "def f():\n    K = 'in.func'\n")
        self.assertEqual(scan.module_string_constants(tree), {"A": "a.b", "B": "m"})

    def test_daemon_pairs(self):
        source = (
            "RUN = 'ham.run'\nMETHOD = 'do_it'\nNUM = 5\nBAD = 'Bad Name'\n"
            "x.execute('a.one', 'm_one')\n"
            "client.call(RUN, METHOD)\n"
            "c.execute_kw('a.two', 'm_two', [1], {})\n"
            "c._json2_call('a.three', 'm_three')\n"
            "c.call_kw('a.four', 'm_four')\n"
            "c._call('a.five', 'm_five')\n"
            "execute('a.six', 'm_six')\n"
            "push_batches_to_odoo(client, logger, rows, 'a.seven', 'm_seven')\n"
            "asyncio.to_thread(client.execute, 'a.eight', 'm_eight')\n"
            "asyncio.to_thread(execute, 'a.nine', 'm_nine')\n"
            "other('a.ten', 'm_ten')\n"
            "handlers[0]('a.sub', 'm_sub')\n"
            "factory()('a.fac', 'm_fac')\n"
            "x.execute('NoDots', 'm_x')\n"
            "x.execute('a.eleven', 'Bad-Method')\n"
            "x.execute('a.twelve')\n"
            "x.execute(NUM, 'm_y')\n"
            "x.execute(BAD, METHOD)\n"
            "x.execute(unknown, 'm_z')\n"
            "x.execute('a.thirteen', unknown)\n"
            "x.execute('a.fourteen', '_private_ok')\n"
            "url = '/json/2/a.url/m_url'\n"
            "furl = f'/json/2/a.furl/m_furl/{x}'\n"
            "x.execute('a.fifteen', 'm_fifteen', 'extra', 'args')\n")
        self.assertEqual(scan.daemon_pairs(source), {
            ("a.one", "m_one"), ("ham.run", "do_it"), ("a.two", "m_two"), ("a.three", "m_three"), ("a.four", "m_four"),
            ("a.five", "m_five"), ("a.six", "m_six"), ("a.seven", "m_seven"), ("a.eight", "m_eight"), ("a.nine", "m_nine"),
            ("a.fourteen", "_private_ok"), ("a.url", "m_url"), ("a.furl", "m_furl"), ("a.fifteen", "m_fifteen")})

    def test_daemon_pairs_batch_helper_scans_every_position_and_syntax_errors_keep_urls(self):
        self.assertEqual(scan.daemon_pairs("helper(push_batches_to_odoo(1, 'x.one', 'm_one', 'x.two', 'm_two'))"),
                         {("x.one", "m_one"), ("x.two", "m_two")})
        self.assertEqual(scan.daemon_pairs("'/json/2/a.b/c' = (\n"), {("a.b", "c")})
        self.assertEqual(scan.daemon_pairs("x = 1"), set())

    def _main(self, args):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = scan.main(args)
        return code, out.getvalue()

    def test_main_check_write_and_skips(self):
        root = self._tree({
            "alpha/__manifest__.py": "{}", "alpha/m.py": self.MODEL.format(name="go"),
            "beta/__manifest__.py": "{}", "beta/m.py": self.MODEL.format(name="go2"),
            "empty/__manifest__.py": "{}",
            "keep/__manifest__.py": "{}", "keep/rpc_exposure.json": json.dumps({"portal": [["a", "b"]]}),
            "stale/__manifest__.py": "{}", "stale/rpc_exposure.json": json.dumps({"internal": {"gone": ["m"]}}),
            "notaddon/m.py": self.MODEL.format(name="zzz"),
        })
        keep_before = open(os.path.join(root, "keep", "rpc_exposure.json")).read()
        self.assertEqual(self._main(["--check", root]), (1, "out of date: alpha, beta, stale\n"))
        self.assertFalse(os.path.exists(os.path.join(root, "alpha", "rpc_exposure.json")))
        self.assertEqual(self._main([root]), (0, "out of date: alpha, beta, stale\n"))
        self.assertFalse(os.path.exists(os.path.join(root, "alpha", "rpc_exposure.json")))
        self.assertEqual(self._main(["--write", root]), (0, "written: alpha, beta, stale\n"))
        self.assertEqual(json.load(open(os.path.join(root, "alpha", "rpc_exposure.json")))["internal"], {"x.y": ["go"]})
        self.assertEqual(json.load(open(os.path.join(root, "beta", "rpc_exposure.json")))["internal"], {"x.y": ["go2"]})
        self.assertEqual(json.load(open(os.path.join(root, "stale", "rpc_exposure.json")))["internal"], {})
        self.assertFalse(os.path.exists(os.path.join(root, "empty", "rpc_exposure.json")))
        self.assertFalse(os.path.exists(os.path.join(root, "notaddon", "rpc_exposure.json")))
        self.assertEqual(open(os.path.join(root, "keep", "rpc_exposure.json")).read(), keep_before)
        self.assertEqual(self._main(["--check", root]), (0, ""))
        self.assertEqual(self._main(["--write", root]), (0, ""))

    def test_main_help_describes_the_tool_by_its_first_docstring_line(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), self.assertRaises(SystemExit) as stop:
            scan.main(["--help"])
        self.assertEqual(stop.exception.code, 0)
        first = scan.__doc__.split("\n")[0]
        self.assertIn(first.split()[0], out.getvalue())
        self.assertIn(first[:40], out.getvalue().replace("\n", " "))
        self.assertNotIn("Run it from a checkout", out.getvalue())

    def test_main_check_and_write_together(self):
        root = self._tree({"alpha/__manifest__.py": "{}", "alpha/m.py": self.MODEL.format(name="go")})
        self.assertEqual(self._main(["--check", "--write", root]), (1, "written: alpha\n"))
        self.assertTrue(os.path.exists(os.path.join(root, "alpha", "rpc_exposure.json")))

    def test_main_rewrites_only_when_the_text_differs(self):
        root = self._tree({"alpha/__manifest__.py": "{}", "alpha/m.py": self.MODEL.format(name="go")})
        self._main(["--write", root])
        path = os.path.join(root, "alpha", "rpc_exposure.json")
        os.utime(path, (1, 1))
        self._main(["--write", root])
        self.assertEqual(os.stat(path).st_mtime, 1)
