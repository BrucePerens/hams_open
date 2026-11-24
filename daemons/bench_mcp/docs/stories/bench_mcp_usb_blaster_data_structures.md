# usb_blaster data structures and functions

The functions of `src/usb_blaster/mod.rs` (the Altera USB-Blaster driver through `openFPGALoader`). Tests are in `src/usb_blaster/tests.rs`.

### `parse_config` [@ANCHOR: bench_mcp:usb_blaster_config]

Reads `program`, `cable` and `allow_flash` of a `usb_blaster` entry into `Config` and refuses a cable other than `usb-blaster`/`usb-blasterII`, an empty program or a non-boolean `allow_flash`. Errors: the message names the key. `allow_flash` defaults to false and only the owner sets it. Concurrency: none; the result never changes.

### `Config / Out / Device / Runner / RealRunner / Spec` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_types]

`Config` holds the entry's keys. `Out` is one program run (exit code, merged stdout and stderr text, timed_out). `Device` is one chain entry (idcode, manufacturer, family, model, irlength). `Runner` is the one way a call reaches `openFPGALoader`: production uses `RealRunner`, tests a fake cable. `Spec` is a tool's name, tier, description and schema. Plain data, no shared state; the trait is called from one tool call at a time (the server serializes calls per instrument).

### `RealRunner::run` [@ANCHOR: bench_mcp:usb_blaster_real_runner]

Starts the configured program with the given arguments as a child of the server, reads its stdout and stderr on two threads and polls for exit. When the time limit passes it ends that one child (`Child::kill`) and returns `timed_out`. Errors: the program cannot start (the text names the next step: install it or set `program`). It never signals any other process.

### `instrument` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_instrument]

Picks the `usb_blaster` entry a call is for: the named one, or the only one. Errors: unknown name, none configured, several without a name.

### `tool_defs` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_tool_defs]

The tools to list: each appears only when an entry at or above its tier exists; the flash tool also needs `allow_flash`. Pure.

### `flag` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_flag]

A boolean argument that may be absent (false) but must not be another type; the error names the key.

### `bitstream` [@ANCHOR: bench_mcp:usb_blaster_file]

Checks the `file` argument: absolute, an extension from the tool's list (`.sof`/`.svf` for SRAM; `.jic`/`.rpd`/`.pof`/`.rbf`/`.bin` for flash), an existing regular file of 1 byte to 128 MiB. Errors name the file and the rule. Reads file metadata only.

### `parse_chain` [@ANCHOR: bench_mcp:usb_blaster_parse_chain]

Turns openFPGALoader's `--detect` text (`index N:` blocks of `idcode`, `manufacturer`, `family`, `model`, `irlength`) into `Device`s. Unknown lines are ignored; empty text gives an empty list.

### `parse_cables` [@ANCHOR: bench_mcp:usb_blaster_parse_cables]

Turns the `--scan-usb` table into cable rows (bus, device, vid:pid, probe, manufacturer). The serial number column is dropped on purpose.

### `chain` [@ANCHOR: bench_mcp:usb_blaster_chain]

Runs `-c <cable> --detect` with a 20 s limit and parses it. Errors (time limit, non-zero exit, empty chain) carry the program's last line and the next step. Used by every tool, so a load never starts against an empty chain.

### `scan` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_scan]

`bench_jtag_scan`: lists cables, then scans the chain. Reads identification registers only. Errors: no cable, no device.

### `sram_load` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_sram_load]

`bench_jtag_sram_load`: validates the file, returns the plan on `dry_run`, refuses without `confirm_sram_volatile`, scans the chain, then runs `-c <cable> -m <file>` (SRAM only, 600 s limit). Never uses `-f`.

### `flash_write` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_flash_write]

`bench_jtag_flash_write`: refused unless `allow_flash`; validates the file; `dry_run` returns the plan; needs `owner_approval` (the owner's quoted yes for this write) and `confirm_flash` equal to `WRITE FLASH <file name>`; scans the chain; runs `-c <cable> -f <file>` (600 s). A failure says the flash may be half written.

### `call_with` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_call_with]

Runs one tool against a given `Runner`: unknown tool, instrument choice, tier check, then dispatch. `call` is the production entry that builds the `RealRunner`.

### `call` [@ANCHOR: bench_mcp:bench_mcp_usb_blaster_call]

The production entry of the module: finds the entry, builds a `RealRunner` from its `program` and runs `call_with`. Errors: no entry, or the program cannot start.

