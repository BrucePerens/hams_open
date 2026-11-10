# SPDX-License-Identifier: AGPL-3.0-or-later
# FULL-SPEED FALLBACK launcher (bench_mcp config "full_speed_fallback": true). It runs the glasgow command line with the gateware's
# IN-packet size set to 64 bytes instead of 512. A Glasgow that links at 12 Mb/s (full speed) has 64-byte bulk endpoints as far as the
# host can use them, but the stock gateware commits 512-byte packets, so every device-to-host read ends in LIBUSB_TRANSFER_OVERFLOW.
# With 64-byte packets the reads work (measured on the revC3 unit, 2026-11-10: about 1 MiB/s). Usage: python glasgow_fs.py <glasgow args>
import sys

import glasgow.gateware.fx2_crossbar as _xbar

_xbar._PACKET_SIZE = 64  # the FX2 crossbar's model of one IN packet; a bitstream built here has its own bitstream id

from glasgow.cli import main
from glasgow.main import run_async

sys.argv[0] = "glasgow"
sys.exit(run_async(main))
