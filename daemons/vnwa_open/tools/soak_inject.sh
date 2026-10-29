#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# soak_inject.sh <run_dir> <generator_example> : failures injected from OUTSIDE the soak during its dry run, so that the soak itself carries no
# test-only code. Only the unit's own power and its own generator command are used; nothing radiates beyond the unit's -17 dBm.
#   after  8 min: the unit's power is cut for 4 s in mid-run          -> unexpected disconnect, recovery power cycle, USB errors, overrun reports
#   after 14 min: generator-off commands for 3 s                      -> a burst with no tone on the reference channels (silent-state class)
#   after 20 min: the PAUSE flag for 60 s                             -> pause, resume, fresh baselines
#   after 26 min: 16 busy-loop threads for 30 s                       -> host load in host.log, overruns if the load causes any
dir=$1; gen=$2
say() { echo "$(date +%s) INJECT $*" | tee -a "$dir/inject.log"; }
sleep 480
say "power off the unit for 4 s"; sudo -n uhubctl -l 3-2 -p 2 -a off >/dev/null; sleep 4; sudo -n uhubctl -l 3-2 -p 2 -a on >/dev/null; say "power on"
sleep 360
say "generator off bursts for 3 s"; end=$((SECONDS+3)); while [ $SECONDS -lt $end ]; do "$gen" off >/dev/null 2>&1; sleep 0.05; done; say "done"
sleep 330
say "PAUSE flag for 60 s"; touch "$dir/PAUSE"; sleep 60; rm -f "$dir/PAUSE"; say "PAUSE removed"
sleep 270
say "16 busy loops for 30 s"; for i in $(seq 16); do timeout 30 sh -c 'while :; do :; done' & done; wait; say "load done"
