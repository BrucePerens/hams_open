#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# soak_run.sh <hours> <run_dir> [cycle_minutes] : the long soak with its wrapper. Copies the soak binary into the run directory (so that rebuilding the
# crate cannot change a running soak), starts it with the hub 3-2 port 2 and the scope at 192.168.10.51:5025, and restarts it after a hang (exit 3) or a
# failed cold reset (exit 4) with a cold reset of the unit in between, until the total time is used up or a STOP file appears in the run directory.
# Flag files in the run directory: PAUSE (the soak waits after the round, so the hardware can be used; remove it to continue) and STOP.
# Build first: `CARGO_TARGET_DIR=<dir> cargo build --release --example soak --example generator` in daemons/vnwa_open.
set -u
hours=${1:?usage: soak_run.sh <hours> <run_dir> [cycle_minutes]}
dir=$(realpath -m "${2:?run_dir}")
cycle=${3:-30}
target=${CARGO_TARGET_DIR:-$HOME/.cache/hams-cargo-target-vna-soak}
mkdir -p "$dir"
cp "$target/release/examples/soak" "$dir/soak_bin"
end=$(python3 -c "import time; print(int(time.time() + $hours * 3600))")
restarts=0
cold_reset() {
  # cold reset with growing off times until the unit is on the bus again
  for off in 4 12 12 30; do
    sudo -n uhubctl -l 3-2 -p 2 -a off >/dev/null 2>&1; sleep "$off"; sudo -n uhubctl -l 3-2 -p 2 -a on >/dev/null 2>&1
    for i in $(seq 25); do sleep 1; lsusb | grep -q 20a0:4118 && break; done
    lsusb | grep -q 20a0:4118 && { sleep 8; return 0; }
  done
  return 1
}
# the unit may be off or stuck at the start: bring it up first
if ! lsusb | grep -q 20a0:4118 || [ "$(lsusb | grep -c 08bb:29c0)" -lt 2 ]; then
  echo "$(date +%s) WRAPPER unit not complete on the bus at the start: cold reset" >> "$dir/soak.log"
  cold_reset || { echo "$(date +%s) WRAPPER the unit does not come back: giving up" >> "$dir/soak.log"; exit 1; }
fi
while [ "$(date +%s)" -lt "$end" ] && [ ! -e "$dir/STOP" ]; do
  left=$(python3 -c "print(max(0.01, ($end - $(date +%s)) / 3600))")
  "$dir/soak_bin" "$left" "$cycle" 3-2:2 192.168.10.51:5025 "$dir" >> "$dir/soak.log" 2>&1
  rc=$?
  echo "$(date +%s) WRAPPER soak exited with status $rc (restarts so far $restarts)" >> "$dir/soak.log"
  [ $rc -eq 0 ] && break
  restarts=$((restarts+1))
  # a stuck unit: cold reset before the next try
  cold_reset || { echo "$(date +%s) WRAPPER the unit does not come back: giving up" >> "$dir/soak.log"; exit 1; }
done
echo "$(date +%s) WRAPPER finished (restarts $restarts)" >> "$dir/soak.log"
