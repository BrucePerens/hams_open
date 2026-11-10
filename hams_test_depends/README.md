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

- `companions.check_entries` ([@ANCHOR: hams_test_depends_check_entries]): the error strings for one module's entries.
- `companions._reaches` ([@ANCHOR: hams_test_depends_reaches]): is a module reachable from another through `test_depends` edges.
- `companions.plan` ([@ANCHOR: hams_test_depends_plan]): transitive companions of the targets; raises `UserError` on bad entries.
- `companions.queue_companions` ([@ANCHOR: hams_test_depends_queue_companions]): marks the uninstalled companions "to install".
- `companions.install_patches` ([@ANCHOR: hams_test_depends_install_patches]): the `post_load` hook that installs the wrappers.
