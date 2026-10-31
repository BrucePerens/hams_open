#!/bin/bash
# Print device/osdevice elements of every BCD object (hex), read-only.
bcd=$1
for o in $(hivexsh "$bcd" <<<'cd Objects
ls'); do
  for e in 11000001 21000001 12000002 12000004; do
    v=$(hivexsh "$bcd" <<<"cd Objects\\$o\\Elements\\$e
lsval" 2>/dev/null) || continue
    [ -n "$v" ] && echo "$o $e: $(hivexsh "$bcd" <<<"cd Objects\\$o\\Elements\\$e
cat Element" 2>/dev/null | head -c 400 | tr -d '\n')"
  done
done
