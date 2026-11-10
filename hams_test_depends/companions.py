# SPDX-License-Identifier: AGPL-3.0-or-later
"""Install the modules a module's tests need, from its manifest `test_depends` key.

`test_depends` lists modules whose presence the module's TESTS require although its runtime `depends` does not (a page measured,
a model or field read by one test). It is NOT `depends_cycle`, the unrelated runtime mechanism for dependency cycles.

Mechanism, for plain `odoo -i/-u <module> --test-enable` (no special runner): `post_load` wraps two Odoo functions once per process.
After `IrModuleModule.button_install` / `button_upgrade` and after every `loading.load_module_graph` call, the wrapper looks at the
modules in state 'to install' / 'to upgrade', reads their `test_depends`, validates it, and calls the original `button_install` on the
companions that are not installed. Odoo's loading loop then loads them (it re-reads module states after each pass). Outside
`--test-enable` nothing happens. Start the server with `--load=base,web,hams_test_depends` to have the wrappers from the very first
step; without it they apply once this module is loaded and still catch every later pass.

Limit: a companion is loaded in the same run but after, or beside, the module that lists it, so a test that needs it at
`at_install` time of the listing module may run first. Tag such tests `post_install` and `-at_install`.
"""
import functools
import logging

from odoo import tools
from odoo.addons.base.models.ir_module import IrModuleModule
from odoo.exceptions import UserError
from odoo.modules import loading
from odoo.modules.module import get_manifest

_logger = logging.getLogger(__name__)
KEY = 'test_depends'


# [@ANCHOR: hams_test_depends_check_entries]
def check_entries(module, entries, manifest_of, exists):
    """Return the error strings for `module`'s `test_depends` `entries` ([] when all are fine).

    `manifest_of(name)` returns a mapping with 'depends' and optionally 'test_depends'; `exists(name)` tells whether the module
    exists. An entry is refused when it is the module itself, already in its real `depends`, unknown, or when the companion
    already reaches `module` through `test_depends` edges alone (a cycle). A companion that depends on `module` at runtime is fine:
    it is installed on top of it."""
    errors = []
    depends = list(manifest_of(module).get('depends') or [])
    for dep in entries:
        if dep == module:
            errors.append("%s: test_depends lists the module itself" % module)
        elif dep in depends:
            errors.append("%s: test_depends lists '%s', which is already in its depends" % (module, dep))
        elif not exists(dep):
            errors.append("%s: test_depends lists '%s', which is not an available module" % (module, dep))
        elif _reaches(dep, module, manifest_of):
            errors.append("%s: test_depends lists '%s', which already reaches %s through test_depends alone (cycle)"
                          % (module, dep, module))
    return errors


# [@ANCHOR: hams_test_depends_reaches]
def _reaches(start, target, manifest_of):
    seen, stack = set(), [start]
    while stack:
        node = stack.pop()
        if node == target:
            return True
        if node in seen:
            continue
        seen.add(node)
        manifest = manifest_of(node)
        stack.extend(manifest.get(KEY) or [])
    return False


# [@ANCHOR: hams_test_depends_plan]
def plan(targets, manifest_of, exists):
    """Companions to install for `targets` (module names), transitively, in first-seen order, excluding the targets themselves.
    Raises UserError listing every bad entry."""
    errors, order, seen, pending = [], [], set(targets), list(targets)
    while pending:
        module = pending.pop(0)
        entries = list(manifest_of(module).get(KEY) or [])
        errors.extend(check_entries(module, entries, manifest_of, exists))
        for dep in entries:
            if dep not in seen and dep != module and exists(dep):
                seen.add(dep)
                order.append(dep)
                pending.append(dep)
    if errors:
        raise UserError("Invalid test_depends manifest entries:\n" + "\n".join(errors))
    return order


# [@ANCHOR: hams_test_depends_queue_companions]
def queue_companions(env, targets=None, manifest_of=get_manifest, button_install=None):
    """In a --test-enable run, mark the uninstalled companions of `targets` (default: modules to install or upgrade) 'to install'.
    Returns the names marked. `button_install` is the unwrapped Odoo method (default: the saved original)."""
    # This module exists to act only in --test-enable runs; it adds test companions and skips no code under test.
    if not tools.config['test_enable']:  # burn-ignore: act only in test runs, nothing skipped
        return []
    Module = env['ir.module.module']  # audit-ignore-search: module records are global, not multi-tenant
    if targets is None:
        targets = Module.search([('state', 'in', ('to install', 'to upgrade'))]).mapped('name')
    known = set(Module.search([]).mapped('name'))
    wanted = plan(list(targets), manifest_of, lambda name: name in known)
    mods = Module.search([('state', '=', 'uninstalled'), ('name', 'in', wanted)])
    if mods:
        (button_install or _ORIGINAL.get('button_install'))(mods)
        _logger.info("test_depends: marked %s to install", ", ".join(sorted(mods.mapped('name'))))
    return sorted(mods.mapped('name'))


_ORIGINAL = {}


# [@ANCHOR: hams_test_depends_install_patches]
def install_patches():
    """post_load: wrap button_install, button_upgrade and loading.load_module_graph once per process."""
    if _ORIGINAL:
        return
    _ORIGINAL['button_install'] = IrModuleModule.button_install
    _ORIGINAL['button_upgrade'] = IrModuleModule.button_upgrade
    _ORIGINAL['load_module_graph'] = loading.load_module_graph

    def after_button(name):
        original = _ORIGINAL[name]

        @functools.wraps(original)
        def wrapper(self, *args, **kwargs):
            result = original(self, *args, **kwargs)
            queue_companions(self.env)
            return result
        return wrapper

    IrModuleModule.button_install = after_button('button_install')
    IrModuleModule.button_upgrade = after_button('button_upgrade')

    @functools.wraps(_ORIGINAL['load_module_graph'])
    def load_module_graph(env, *args, **kwargs):
        queue_companions(env)
        return _ORIGINAL['load_module_graph'](env, *args, **kwargs)

    loading.load_module_graph = load_module_graph
