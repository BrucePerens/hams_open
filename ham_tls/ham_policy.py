# SPDX-License-Identifier: AGPL-3.0-or-later
"""Engine-independent ham-side policy for the hams.com Net proxy: which sites may be reached, and the
station-identification log Part 97 requires. Any proxy engine calls these two functions."""

import time


# [@ANCHOR: ham_policy:whitelist]
class Whitelist:
    """Permitted destinations: every `.ham` name plus the listed domains ('example.org' exact, '*.example.org' subdomains)."""

    def __init__(self, entries):
        self.exact = {e.lower() for e in entries if not e.startswith("*.")}
        self.suffixes = tuple("." + e[2:].lower() for e in entries if e.startswith("*."))

    def permits(self, host):
        host = host.lower().rstrip(".")
        return host.endswith(".ham") or host in self.exact or host.endswith(self.suffixes)


# [@ANCHOR: ham_policy:log_connection]
def log_connection(stream, callsign, host, suite, now=None):
    """Write one station-identification line (UTC time, callsign, destination, suite); a callsign is required."""
    if not callsign:
        raise ValueError("a ham link connection needs the station callsign")
    stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now if now is not None else time.time()))
    stream.write("%s %s -> %s %s\n" % (stamp, callsign.upper(), host, suite))
