#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# soak_inject.sh <run_dir> <generator_example> [short] : failures injected from OUTSIDE the soak during a dry run, so that the soak itself carries no
# test-only code. Only the unit's own power and its own generator command are used; nothing radiates beyond the unit's -17 dBm.
# 30-minute schedule: at 8 min the unit's power is cut for 4 s (unexpected disconnect, recovery power cycle, USB errors); at 14 min generator-off commands
# for 3 s; at 20 min the PAUSE flag for 60 s; at 26 min 16 busy-loop threads for 30 s.
# "short" (15 minutes): disconnect at 2.5 min; generator-off bursts of 2 s every 11 s from 4 to 7 min (so that some fall inside a sweep: a burst with no
# tone on the reference channels, the silent-state class); PAUSE at 8 min for 40 s.
dir=$1; gen=$2
say() { echo "$(date +%s) INJECT $*" | tee -a "$dir/inject.log"; }
burst() { local end=$((SECONDS+$1)); while [ $SECONDS -lt $end ]; do "$gen" off >/dev/null 2>&1; sleep 0.05; done; }
cut_power() { sudo -n uhubctl -l 3-2 -p 2 -a off >/dev/null; sleep 4; sudo -n uhubctl -l 3-2 -p 2 -a on >/dev/null; }
if [ "$3" = short ]; then
  sleep 150; say "power off the unit for 4 s"; cut_power; say "power on"
  sleep 90
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16; do say "generator off burst 2 s ($i)"; burst 2; sleep 9; done
  sleep 20
  say "PAUSE flag for 40 s"; touch "$dir/PAUSE"; sleep 40; rm -f "$dir/PAUSE"; say "PAUSE removed"
  exit 0
fi
sleep 480
say "power off the unit for 4 s"; cut_power; say "power on"
sleep 360
say "generator off bursts for 3 s"; burst 3; say "done"
sleep 330
say "PAUSE flag for 60 s"; touch "$dir/PAUSE"; sleep 60; rm -f "$dir/PAUSE"; say "PAUSE removed"
sleep 270
say "16 busy loops for 30 s"; for i in $(seq 16); do timeout 30 sh -c 'while :; do :; done' & done; wait; say "load done"
