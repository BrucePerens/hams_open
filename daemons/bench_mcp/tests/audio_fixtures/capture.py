# SPDX-License-Identifier: AGPL-3.0-or-later
"""Record the sound graph of this machine for the audio tests: the
nodes, devices and default-device metadata from pw-dump, trimmed to the
fields the audio module reads, and the card tables from /proc/asound.

usage: capture.py OUTDIR

Host names, serial numbers and Bluetooth addresses are replaced.
"""
import json
import os
import re
import subprocess
import sys

KEEP_NODE_PARAMS = ("EnumFormat",)
KEEP_DEVICE_PARAMS = ("Route",)


def scrub(text):
    text = re.sub(r"([0-9A-Fa-f]{2}[:_]){5}[0-9A-Fa-f]{2}", "00:00:00:00:00:00", text)
    host = subprocess.run(["hostname"], capture_output=True, text=True).stdout.strip()
    if host:
        text = text.replace(host, "devbox")
    return text


def trim(obj):
    kind = obj["type"].split(":")[-1]
    info = obj.get("info", {})
    if kind == "Node":
        params = {k: v for k, v in info.get("params", {}).items() if k in KEEP_NODE_PARAMS}
        return {"id": obj["id"], "type": obj["type"], "info": {"state": info.get("state"), "props": info.get("props", {}), "params": params}}
    if kind == "Device":
        params = {k: v for k, v in info.get("params", {}).items() if k in KEEP_DEVICE_PARAMS}
        return {"id": obj["id"], "type": obj["type"], "info": {"props": info.get("props", {}), "params": params}}
    if kind == "Metadata":
        keep = [m for m in obj.get("metadata", []) if str(m.get("key", "")).startswith("default.")]
        return {"id": obj["id"], "type": obj["type"], "metadata": keep}
    return None


def main(out):
    os.makedirs(out, exist_ok=True)
    dump = json.loads(subprocess.run(["pw-dump"], capture_output=True, text=True, check=True).stdout)
    kept = [t for t in (trim(o) for o in dump) if t]
    kept = [t for t in kept if not t["type"].endswith("Node") or str(t["info"]["props"].get("media.class", "")).startswith("Audio/")]
    text = scrub(json.dumps(kept, indent=1, sort_keys=True))
    with open(os.path.join(out, "pw-dump.json"), "w") as f:
        f.write(text)
    for name in ("cards", "pcm"):
        src = "/proc/asound/" + name
        if os.path.exists(src):
            with open(os.path.join(out, name), "w") as f:
                f.write(scrub(open(src).read()))
    for card in sorted(os.listdir("/proc/asound")):
        usbid = os.path.join("/proc/asound", card, "usbid")
        if re.fullmatch(r"card\d+", card) and os.path.exists(usbid):
            os.makedirs(os.path.join(out, card), exist_ok=True)
            with open(os.path.join(out, card, "usbid"), "w") as f:
                f.write(open(usbid).read())


if __name__ == "__main__":
    main(sys.argv[1])
