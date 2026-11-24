#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Refuse when the build's kernel config differs from AREDN's published one on options that change kernel struct layouts.
usage: check_kconfig.py PUBLISHED_config.buildinfo TREE_openwrt_.config TREE_linux_.config
Why: with CONFIG_NET_NS=y, struct genl_info carries a `_net` field that stock AREDN kernels (NET_NS off) do not have; the inline
genlmsg_reply() in a module built that way reads a shifted field and every Jool netlink reply fails with -ECONNREFUSED (-111)."""
import re
import sys

# kernel symbols that change layouts of structs Jool and netfilter modules touch (genl_info, net, sk_buff, nf_conn, netns refs)
LAYOUT = ("NET_NS", "NETFILTER_EGRESS", "NF_CONNTRACK_EVENTS", "NF_CONNTRACK_LABELS", "NF_CONNTRACK_MARK", "NF_CONNTRACK_ZONES",
          "NF_CONNTRACK_TIMESTAMP", "NF_CONNTRACK_SECMARK", "NETWORK_SECMARK", "SKB_EXTENSIONS", "BRIDGE_NETFILTER",
          "NET_CLS_ACT", "NET_SCHED", "XFRM", "SECURITY_NETWORK", "NETFILTER_NETLINK_ACCT", "NET_NS_REFCNT_TRACKER")


# [@ANCHOR: aredn_jool:check_kconfig_parse]
def parse(path):
    cfg = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = re.match(r"(?:# )?(CONFIG_\w+)(?:=(.*)| is not set)\s*$", line)
            if m:
                cfg[m.group(1)] = m.group(2) or "n"
    return cfg


# [@ANCHOR: aredn_jool:check_kconfig_main]
def main(argv):
    pub, tree, linux = parse(argv[1]), parse(argv[2]), parse(argv[3])
    bad = []
    for key, val in pub.items():
        if key.startswith("CONFIG_KERNEL_") and tree.get(key, "n") != val:
            bad.append("%s published %s, build %s" % (key, val, tree.get(key, "n")))
    # CONFIG_KERNEL_NAMESPACES published off means the stock kernel has no NET_NS
    if pub.get("CONFIG_KERNEL_NAMESPACES", "n") == "n" and linux.get("CONFIG_NET_NS", "n") != "n":
        bad.append("CONFIG_NET_NS=%s in the build but the release kernel has namespaces off" % linux["CONFIG_NET_NS"])
    print("layout options in the build kernel: " + " ".join("%s=%s" % (k, linux.get("CONFIG_" + k, "n")) for k in LAYOUT))
    for b in bad:
        print("MISMATCH " + b)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
