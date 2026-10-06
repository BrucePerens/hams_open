# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The static rule that fails when a new code path hands a raw helpdesk ticket, message or incident body
to an AI reader (docs/security/TICKET_PROMPT_INJECTION.md). Pure Python with no Odoo import, so the
hams_com daemon test loads this one file by path.

This repository enforces rules with AST scans and unit tests (there is no CI), so this is the same kind
of check. It looks at two shapes, both of which are how an AI reader gets text in this codebase:

1. An Odoo model method named ``mcp_*`` (what the MCP servers and AI daemons call) on a ticket-like model
   that reads a free-text attribute (description, body, name, subject...) must use the shared filter in
   the same method (``ut.``, ``safe_view``, ``wrap_untrusted``, ``sanitize_any``, ``scan_text``).
2. A daemon-side ``client.execute(<ticket-like model>, "read" | "search_read", ..., fields)`` that asks for
   a free-text field (or for every field) must sit in a function that also goes through the safe view
   (``mcp_safe_read``, ``safe_text``, ``wrap_untrusted``...). Field lists are resolved through module
   constants such as ``_TICKET_FIELDS + ["partner_id"]``.

A false positive is silenced with ``# ticket-ai-ignore: <reason>`` on the line of the call or the def.
It is a heuristic AST scan, not dataflow analysis; the filter itself is the defence, this keeps a new
path from skipping it by accident.
"""
import ast
import os

SKIP_DIRS = {"__pycache__", "node_modules", ".git", "hams_shared", "tests", "worktrees", ".claude", "site-packages", "venv", ".venv", "target"}

# Models whose text a stranger (or a log line a stranger shaped) wrote.
TICKET_LIKE_MODELS = {
    "hams_helpdesk.ticket",
    "pager.incident",
    "mail.message",
    "ham.simulated.band.report",
    "hams_helpdesk.ticket.suspicion",
    "event.event",  # scraped third-party descriptions
}
TEXT_FIELDS = {
    "name", "description", "body", "subject", "callsign", "note", "email_from", "partner_name",
    "message_ids", "display_name", "raw_original", "reporter_text", "transcript",
}
READ_METHODS = {"read", "search_read", "search_fetch"}
SAFE_MARKERS = {
    "ut", "untrusted_text", "safe_view", "mcp_safe_read", "safe_text", "wrap_untrusted", "sanitize_any",
    "scan_text", "sanitize_html", "_safe_rows", "safe_event", "safe_events", "filter_ai_output",
    "safe_fields", "_hostile_event", "_event_block",
}
# Shape 2 only applies to a file that talks to a model or serves one (a plain sync script that reads event
# names to de-duplicate them is not an AI reader).
AI_FILE_MARKERS = (
    "CLAUDE_COMMAND", "_call_claude", "_call_agy", "AGY_COMMAND", "generativelanguage", "anthropic",
    "MCPServer(", "FastMCP(", "@mcp.tool",
)
IGNORE_TAG = "ticket-ai-ignore:"


def _py_files(root, daemon_only=False):
    for dirpath, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS and not d.startswith(".")]
        for fname in files:
            if not fname.endswith(".py") or fname.startswith("test_"):
                continue
            path = os.path.join(dirpath, fname)
            if daemon_only and not any(part in ("daemon", "daemons") for part in os.path.relpath(path, root).split(os.sep)):
                continue
            yield path


def _constants(tree):
    """Module-level NAME = "str" / [..] / list + list, resolved to Python values where simple."""
    consts = {}

    def value(node):
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, (ast.List, ast.Tuple)):
            items = [value(e) for e in node.elts]
            return None if any(i is None for i in items) else items
        if isinstance(node, ast.Name):
            return consts.get(node.id)
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            a, b = value(node.left), value(node.right)
            return a + b if isinstance(a, list) and isinstance(b, list) else None
        return None

    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            resolved = value(node.value)
            if resolved is not None:
                consts[node.targets[0].id] = resolved
    return consts, value


def _markers(node):
    found = set()
    for sub in ast.walk(node):
        if isinstance(sub, ast.Name):
            found.add(sub.id)
        elif isinstance(sub, ast.Attribute):
            found.add(sub.attr)
        elif isinstance(sub, ast.Constant) and isinstance(sub.value, str):
            found.add(sub.value)  # client.execute("hams_helpdesk.ticket", "mcp_safe_read", ...)
    return found


def _ignored(lines, *linenos):
    return any(0 < n <= len(lines) and IGNORE_TAG in lines[n - 1] for n in linenos)


def check_source(source, filename="<src>"):
    """List of (line, message) violations in one Python source."""
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return []
    lines = source.splitlines()
    consts, value = _constants(tree)
    parents = {}
    for parent in ast.walk(tree):
        for child in ast.iter_child_nodes(parent):
            parents[child] = parent
    problems = []

    def enclosing_function(node):
        while node in parents:
            node = parents[node]
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                return node
        return tree

    # Shape 1: mcp_* methods on a ticket-like Odoo model.
    for cls in [n for n in ast.walk(tree) if isinstance(n, ast.ClassDef)]:
        models = set()
        for stmt in cls.body:
            if isinstance(stmt, ast.Assign) and any(isinstance(t, ast.Name) and t.id in ("_name", "_inherit") for t in stmt.targets):
                got = value(stmt.value)
                if isinstance(got, str):
                    models.add(got)
                elif isinstance(got, list):
                    models.update(g for g in got if isinstance(g, str))
        if not models & TICKET_LIKE_MODELS:
            continue
        for fn in [n for n in cls.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and n.name.startswith("mcp_")]:
            if _ignored(lines, fn.lineno, fn.lineno - 1):
                continue
            reads = {n.attr for n in ast.walk(fn) if isinstance(n, ast.Attribute) and n.attr in TEXT_FIELDS and isinstance(n.ctx, ast.Load)}
            if reads and not (_markers(fn) & SAFE_MARKERS):
                problems.append((fn.lineno, "%s() reads %s of a ticket-like model without the shared untrusted-text filter" % (fn.name, sorted(reads))))

    # Shape 2: execute(<ticket-like model>, read|search_read, ..., fields) with text fields, no safe view.
    is_ai_file = any(token in source for token in AI_FILE_MARKERS)
    for call in [n for n in ast.walk(tree) if isinstance(n, ast.Call)] if is_ai_file else []:
        func = call.func
        if not (isinstance(func, ast.Attribute) and func.attr in ("execute", "execute_kw", "call")):
            continue
        if len(call.args) < 2:
            continue
        model, method = value(call.args[0]), value(call.args[1])
        if model not in TICKET_LIKE_MODELS or method not in READ_METHODS:
            continue
        if _ignored(lines, call.lineno):
            continue
        fields_node = call.args[3] if len(call.args) > 3 else next((k.value for k in call.keywords if k.arg == "fields"), None)
        fields = value(fields_node) if fields_node is not None else None
        text_read = fields is None or bool(set(fields) & TEXT_FIELDS) or not fields  # unknown or empty means "everything"
        if not text_read:
            continue
        fn = enclosing_function(call)
        if _markers(fn) & SAFE_MARKERS:
            continue
        problems.append((call.lineno, "reads %s text from %s for a reader without the safe view" % (method, model)))
    return problems


def scan_tree(root, daemon_only=False):
    found = []
    for path in _py_files(root, daemon_only):
        with open(path, encoding="utf-8") as handle:
            for line, message in check_source(handle.read(), path):
                found.append("%s:%d: %s" % (os.path.relpath(path, root), line, message))
    return found


