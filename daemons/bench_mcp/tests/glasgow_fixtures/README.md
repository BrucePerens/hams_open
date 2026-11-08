Recordings for the Glasgow module (`src/glasgow/`).

* `voltage_idle.txt`, `list.txt`, `list_stderr.txt`: recorded from a revC3 on 2026-11-08 with the pinned `glasgow` (commit 70cb38c9) over USB, the
  serial replaced by a placeholder. Real device output.
* `jtag_scan_log.txt`: the log lines of the pinned applet's source (`logger.info` formats in `jtag_probe`), NOT recorded from a device.
* `capture.vcd`: written by hand in the layout pyvcd uses (the writer `glasgow run analyzer` uses): a 1 kHz square wave on `clk` (500 us half period) and a 2-pulse burst on `burst`.
* `uart_analyzer.csv`: the format in the uart-analyzer applet's help text.
The helper script (`src/glasgow/ops.py`) is run for real by the tests under a Python harness with fake interface objects; no Glasgow
package is needed to run the tests.
