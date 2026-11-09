# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Finds the public methods of an addon's models by reading its source, and writes the transitional `rpc_exposure.json`.

Run it from a checkout: `python3 hams_rpc_guard/exposure_scan.py --write .` (every managed addon under `.`) or
`--check .` (exit 1 when a file is out of date). It needs no database: the addon's Python is parsed, never imported.
A method counts when it is a plain `def` in a class that sets `_name` or `_inherit`, its name does not start with `_`,
it is not a `staticmethod`/`classmethod` (core refuses those over RPC), not `@api.private` (core refuses it), and not a
standard ORM method (`write`, `search_read`, ... : the web client needs those and core behaviour is kept).
"""
import argparse
import ast
import json
import os
import re
import sys

from odoo.models import BaseModel

SKIP_DIRS = {"tests", "test", "tools", "scripts", "migrations", "static", "daemon", "daemons", "node_modules", "__pycache__", "docs"}
MARK_NAMES = {"public_rpc"}
CALLERS = {"execute", "call", "execute_kw", "_json2_call", "call_kw", "_call"}
MODEL_NAME = re.compile(r"^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$")
METHOD_NAME = re.compile(r"^[a-z_][a-z0-9_]*$")
JSON2_URL = re.compile(r"/json/2/([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)/([a-z_][a-z0-9_]*)")


# [@ANCHOR: hams_rpc_guard:scan_orm_names]
def orm_names():
    """The names defined by Odoo's `BaseModel`: the standard ORM methods that keep core behaviour."""
    return set(dir(BaseModel))


# [@ANCHOR: hams_rpc_guard:scan_decorator_names]
def _decorator_names(node):
    names = set()
    for decorator in node.decorator_list:
        target = decorator.func if isinstance(decorator, ast.Call) else decorator
        if isinstance(target, ast.Attribute):
            names.add(target.attr)
        elif isinstance(target, ast.Name):
            names.add(target.id)
    return names


# [@ANCHOR: hams_rpc_guard:scan_string_values]
def _string_values(node):
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return [node.value]
    if isinstance(node, (ast.List, ast.Tuple)):
        return [element.value for element in node.elts if isinstance(element, ast.Constant) and isinstance(element.value, str)]
    return []


# [@ANCHOR: hams_rpc_guard:scan_model_of_class]
def model_of_class(node):
    """The model name a class body defines or extends: `_name`, else the first `_inherit`; None if it names neither."""
    name, inherit = None, []
    for statement in node.body:
        if isinstance(statement, ast.Assign) and len(statement.targets) == 1 and isinstance(statement.targets[0], ast.Name):
            if statement.targets[0].id == "_name":
                values = _string_values(statement.value)
                name = values[0] if values else None
            elif statement.targets[0].id == "_inherit":
                inherit = _string_values(statement.value)
    return name or (inherit[0] if inherit else None)


# [@ANCHOR: hams_rpc_guard:scan_source]
def scan_source(source, standard):
    """`{model: {"public": [methods], "marked": [methods]}}` of one Python source text.

    `public` are the public non-standard methods; `marked` are those among them already carrying `@public_rpc`."""
    found = {}
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return found
    for node in ast.walk(tree):
        if not isinstance(node, ast.ClassDef):
            continue
        model = model_of_class(node)
        if not model:
            continue
        for statement in node.body:
            if not isinstance(statement, (ast.FunctionDef, ast.AsyncFunctionDef)) or statement.name.startswith("_"):
                continue
            decorators = _decorator_names(statement)
            if decorators & {"staticmethod", "classmethod", "private"} or statement.name in standard:
                continue
            entry = found.setdefault(model, {"public": set(), "marked": set()})
            entry["public"].add(statement.name)
            if decorators & MARK_NAMES:
                entry["marked"].add(statement.name)
    return found


# [@ANCHOR: hams_rpc_guard:scan_addon]
def scan_addon(path, standard=None):
    """`{model: sorted public methods}` for the addon directory `path`, skipping tests, tools, daemons and static files."""
    standard = orm_names() if standard is None else standard
    result = {}
    for directory, subdirectories, files in os.walk(path):
        subdirectories[:] = sorted(d for d in subdirectories if d not in SKIP_DIRS and not d.startswith("."))
        for filename in sorted(files):
            if not filename.endswith(".py") or filename.startswith("test_"):
                continue
            with open(os.path.join(directory, filename), encoding="utf-8") as handle:
                for model, entry in scan_source(handle.read(), standard).items():
                    result.setdefault(model, set()).update(entry["public"] - entry["marked"])
    return {model: sorted(methods) for model, methods in sorted(result.items()) if methods}


# [@ANCHOR: hams_rpc_guard:scan_render]
def render(scanned, existing=None):
    """The text of an addon's `rpc_exposure.json`: the scanned `internal` section, every other section of `existing` kept."""
    document = {key: value for key, value in (existing or {}).items() if key != "internal"}
    document["comment"] = document.get(
        "comment",
        "Transitional allowlist generated by hams_rpc_guard/exposure_scan.py: every public method this addon's models had "
        "when it was generated. Shrink it by making methods private (leading underscore or @api.private) or by "
        "declaring them with @public_rpc; never add a method here by hand without a reviewer.")
    document["internal"] = scanned
    ordered = {key: document[key] for key in ("comment", "internal", "portal", "stock", "clients") if key in document}
    return json.dumps(ordered, indent=2, sort_keys=False) + "\n"


# [@ANCHOR: hams_rpc_guard:scan_managed_addons]
def managed_addons(root):
    """Addon directory names under `root` (every directory with a manifest): a repository's addons are all ours."""
    return [entry for entry in sorted(os.listdir(root)) if os.path.isfile(os.path.join(root, entry, "__manifest__.py"))]


# [@ANCHOR: hams_rpc_guard:scan_daemon_pairs]
def daemon_pairs(source):
    """The `(model, method)` pairs a daemon source file calls: `x.execute/call/execute_kw/_json2_call("model", "method", ...)`
    with two literal strings, and literal `/json/2/<model>/<method>` URLs (also inside f-strings' constant parts)."""
    pairs = set()
    for match in JSON2_URL.finditer(source):
        pairs.add((match.group(1), match.group(2)))
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return pairs
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and len(node.args) >= 2:
            first, second = node.args[0], node.args[1]
            function = node.func
            name = function.attr if isinstance(function, ast.Attribute) else (function.id if isinstance(function, ast.Name) else "")
            if (name in CALLERS and isinstance(first, ast.Constant) and isinstance(second, ast.Constant)
                    and isinstance(first.value, str) and isinstance(second.value, str)
                    and MODEL_NAME.match(first.value) and METHOD_NAME.match(second.value)):
                pairs.add((first.value, second.value))
    return pairs


# [@ANCHOR: hams_rpc_guard:scan_main]
def main(argv=None):
    """Command line: `--write ROOT` writes the files, `--check ROOT` lists out-of-date ones and exits 1. Returns the exit code."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("root")
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    standard = orm_names()
    stale = []
    for name in managed_addons(args.root):
        directory = os.path.join(args.root, name)
        target = os.path.join(directory, "rpc_exposure.json")
        existing = None
        if os.path.isfile(target):
            with open(target, encoding="utf-8") as handle:
                existing = json.load(handle)
        scanned = scan_addon(directory, standard)
        if not scanned and existing is not None and "internal" not in existing:
            continue
        text = render(scanned, existing) if scanned or existing else None
        current = open(target, encoding="utf-8").read() if os.path.isfile(target) else None
        if text is not None and text != current:
            stale.append(name)
            if args.write:
                with open(target, "w", encoding="utf-8") as handle:
                    handle.write(text)
    if stale:
        sys.stdout.write("%s: %s\n" % ("written" if args.write else "out of date", ", ".join(stale)))
    return 1 if (stale and args.check) else 0


if __name__ == "__main__":
    sys.exit(main())
