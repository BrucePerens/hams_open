# Hams Test Companions (`test_depends`)

Lets any Odoo module say, in its `__manifest__.py`, which other modules its **tests** need installed although its runtime
`depends` does not list them:

```python
'depends': ['website'],
# ham_dns's contrast test scans /my/dns in every colour scheme, which needs theme_hams' stylesheet.
'test_depends': ['theme_hams'],
```

This is not `depends_cycle`, which is an unrelated runtime mechanism for dependency cycles. Odoo ignores unknown manifest keys,
so the key costs nothing in production.

## How it works with plain Odoo

Install `hams_test_depends`, then run `odoo -d DB -i my_module --test-enable` (or `-u`). When `--test-enable` is on, the module
wraps `button_install`, `button_upgrade` and `loading.load_module_graph` once per process (`post_load`). It reads `test_depends`
of every module in state "to install" / "to upgrade", checks it, and marks uninstalled companions (and their own companions) "to
install"; Odoo's loading loop then installs them in the same run. Outside `--test-enable` it does nothing. Add
`--load=base,web,hams_test_depends` to apply the wrappers from the first step.

Entries are refused with a `UserError` naming each one when they are the module itself, already in `depends`, not an available
module, or would close a cycle of `test_depends` edges alone (A's tests need B and B's tests need A). A companion that depends
on the module at runtime is fine: it is installed on top of the module.

Limit: a companion is loaded after or beside the module that lists it, so tag a test that needs it `post_install` and
`-at_install`.

## Related tooling (Hams repositories)

`hams_shared/tools/test_impact.py` treats each entry as an extra reverse edge (a change to the companion selects the dependent's
tests); `hams_shared/tools/check_test_depends.py` is the lint auditor for the key; `devbox_tools/asset_gate.py` installs the
entries in remote runs.

## Internal functions

All five live in `companions.py` and are covered by `tests/test_companions.py`. None touches hardware or the network; the only side effects are marking uninstalled Odoo modules "to install" and replacing three Odoo callables once per process, and only in a `--test-enable` run.

### `check_entries(module, entries, manifest_of, exists)` [@ANCHOR: hams_test_depends_check_entries]

Validates one module's `test_depends` list and returns the error strings (an empty list when every entry is fine). It never raises and never reads the database: `manifest_of(name)` must return a mapping with `depends` and optionally `test_depends`, and `exists(name)` says whether a module is available.

| Argument | Meaning |
|---|---|
| `module` | Name of the module whose entries are checked. |
| `entries` | The module's `test_depends` names. |
| `manifest_of` | Callable giving a module's manifest mapping. |
| `exists` | Callable telling whether a module name is an available module. |

Each entry produces at most one error, checked in this order: the module itself, a name already in the module's real `depends`, a name that is not an available module, and a companion that already reaches `module` through `test_depends` edges alone (a cycle, found with `_reaches`). A companion that depends on `module` at runtime is accepted: it is installed on top of the module. Pure apart from the callables it is given. Tests: `test_companions.py`.

### `_reaches(start, target, manifest_of)` [@ANCHOR: hams_test_depends_reaches]

Answers whether `target` can be reached from `start` by following `test_depends` edges only (runtime `depends` edges are ignored on purpose, so a companion that depends on the module at runtime does not count as a cycle). It is an iterative depth-first walk with a seen set, so a loop already present in the manifests cannot make it run forever; `start == target` is true at once. Returns a bool and raises nothing. `check_entries` uses it for the cycle error. Tests: `test_companions.py`.

### `plan(targets, manifest_of, exists)` [@ANCHOR: hams_test_depends_plan]

Computes the companions that must be installed for a list of module names. It walks the targets breadth first, reads each module's `test_depends`, runs `check_entries` on it, and appends each new available companion once, in first-seen order, queueing it so its own companions are followed too (transitive). The targets themselves are never in the result, and an unknown name is not followed. Returns the ordered list of names. If any module had a bad entry it raises `UserError` ("Invalid test_depends manifest entries:") listing every error found across the whole walk, not only the first, so a broken manifest is fixed in one pass. Pure, no database access. Tests: `test_companions.py`.

### `queue_companions(env, targets=None, manifest_of=get_manifest, button_install=None)` [@ANCHOR: hams_test_depends_queue_companions]

In a `--test-enable` run, marks the uninstalled companions of `targets` "to install" and returns their names, sorted. Outside `--test-enable` it returns `[]` immediately and does nothing, so production is untouched.

| Argument | Meaning |
|---|---|
| `env` | An Odoo environment; used to search `ir.module.module`. |
| `targets` | Module names to read; by default every module in state "to install" or "to upgrade". |
| `manifest_of` | Manifest reader (default Odoo's `get_manifest`), replaceable in tests. |
| `button_install` | The unwrapped Odoo install method; by default the one saved by `install_patches`. |

It lists all module names to know which exist, calls `plan`, then searches the modules in that plan that are still `uninstalled` and passes them to the original `button_install` (not the wrapper, so it cannot recurse), logging the names at info level. Errors: `UserError` from `plan` for a bad entry. The searches are global, not per-tenant, because module records are global. Tests: `test_companions.py`.

### `install_patches()` [@ANCHOR: hams_test_depends_install_patches]

The `post_load` hook (exported from `__init__.py`). The first call saves `IrModuleModule.button_install`, `button_upgrade` and `loading.load_module_graph` in a module-level dict, then replaces the first two with wrappers that run the original and then call `queue_companions` on the same environment, and replaces `load_module_graph` with a wrapper that calls `queue_companions` before the original, so companions are queued ahead of each loading pass. A second call returns at once, so the wrappers are installed once per process and never stack. It takes no arguments, returns nothing and does not raise. The wrappers act only in `--test-enable` runs (the check is inside `queue_companions`). Starting the server with `--load=base,web,hams_test_depends` installs them from the first step. Tests: `test_companions.py`.
