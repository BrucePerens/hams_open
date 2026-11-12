---
title: bench_scope_setup
summary: Save, recall, export and restore the scope's whole setup.
see_also: [tools/bench_status, instruments/scope]
tools: [bench_scope_setup]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_setup

Saves the scope's complete front-panel setup in one of its 20 internal slots (`save_slot`, `*SAV`), recalls a slot (`recall_slot`, `*RCL`; slot 0 is the default setup), exports the setup to a file in the output directory (`export`, the `PNSU?` setup block) and restores it from such a file (`import`, a file name without a path). The scope's USB file functions (`STPN`, `RCPN`) are not used. A recall or import changes every setting of the scope, so use it only with the bench in a known state. Commands are from the SDS1000X-E programming guide; not yet checked on firmware 1.3.27 (the setup block's format, binary or XML, is the scope's own and is stored unchanged).

## Returns

`saved_slot`, `recalled_slot`, `exported` (`file`, `bytes`), `imported_bytes`, `after` (the timebase and trigger mode read back) and a `note`.

## Arguments

<!-- arguments -->

* `save_slot`: 1 to 20.
* `recall_slot`: 0 to 20; 0 recalls the default setup.
* `export`: true: write the setup block to a `setup_<time>.set` file in the output directory.
* `import`: the name of a file in the output directory, as `export` wrote it.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
