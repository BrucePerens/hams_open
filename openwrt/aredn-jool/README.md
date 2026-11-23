# Jool for stock AREDN firmware

Builds `kmod-jool-netfilter`, `jool-tools-netfilter`, the libnl packages and `kmod-crypto-md5` from AREDN's own release tree, so the
modules load and work on the UNMODIFIED pre-built AREDN image of a target (no custom firmware, nothing for AREDN to ship). Nothing is published.

    openwrt/aredn-jool/build_aredn_jool.sh -t x86-64 [-r 4.26.10.0] [-w WORK] [-o OUT] [-j JOBS] [-k]

Targets (AREDN config names): `x86-64 ath79-generic ath79-mikrotik ath79-nand ipq40xx-generic ipq40xx-mikrotik mediatek-filogic
ramips-mt7621 ramips-mt76x8`. One target takes about 1 to 3 hours and 40 to 60 GB (kernel, toolchain); the tree is deleted afterwards
(`-k` keeps it); the apks land in `OUT/<target>/` with a `VERIFIED` file. Run one target at a time.

What it does: clones `github.com/aredn/aredn` at the tag, `make TARGET=... prepare`, adds only `CONFIG_PACKAGE_kmod-jool-netfilter=m`,
`jool-tools-netfilter=m`, `kmod-crypto-md5=m`, `libnl=m`, builds the kernel, then **refuses (exit 3)** unless all of these hold:

1. the release's kernel hash is read from the published `packages/kernel-<ver>~<hash>-r1.apk` name;
2. `check_kconfig.py`: every `CONFIG_KERNEL_*` in the published `config.buildinfo` equals the tree's, and the kernel has `CONFIG_NET_NS` off when the release has
   `CONFIG_KERNEL_NAMESPACES` off (cause below);
3. `compare_modabi.py`: at least 3 same-named kernel modules of the release (extracted from its published kmod apks) are identical to the tree's in vermagic,
   every modversion CRC and all code and data sections. AREDN builds without `CONFIG_MODVERSIONS`, so only this proves the same kernel config and compiler.

Only then is the release hash written to the tree's `.vermagic` (the tree hash differs because the extra Jool packages change the kernel option set; the
modules proved identical) and Jool built; the resulting kmod must depend on `kernel=<ver>~<release hash>`.

## Why the OpenWrt SDK build failed (`genlmsg_reply` -111)

A kmod built with the stock OpenWrt 25.12 SDK loads on AREDN 4.26.10.0 (the vermagic matches) but every Jool netlink reply fails with
`genlmsg_reply() failed (errcode -111)`. Cause (2026-11-23): the SDK x86-64 kernel has `CONFIG_NET_NS=y`, AREDN's has it off (also `NETFILTER_EGRESS`,
`NF_CONNTRACK_EVENTS`, `NF_CONNTRACK_LABELS` differ: about 1500 option differences). `genlmsg_reply()` is an inline function: `genlmsg_unicast(genl_info_net(info), skb, info->snd_portid)`,
and `genl_info_net()` reads `info->_net`, a `possible_net_t` that exists only with NET_NS. In the module built with NET_NS the field is read at an offset that
holds something else in AREDN's `struct genl_info`, so the unicast goes to a bogus netns and fails with ECONNREFUSED. Without modversions nothing checks
struct layout at load time. Any module built against a kernel whose layout-affecting options differ has this class of bug: hence checks 2 and 3.

## Helper functions

### `[@ANCHOR: aredn_jool:check_kconfig_parse]` `parse`
Reads a kernel or OpenWrt config file into `{CONFIG_X: value}`; `# CONFIG_X is not set` becomes `n`.

### `[@ANCHOR: aredn_jool:check_kconfig_main]` `main`
Compares the published `config.buildinfo` with the tree's config and the NET_NS state; exit 1 on any mismatch.

### `[@ANCHOR: aredn_jool:compare_modabi_sections]` `sections`
Splits an ELF kernel module into its named sections (32 or 64 bit, either endianness).

### `[@ANCHOR: aredn_jool:compare_modabi_abi]` `abi`
Returns a module's vermagic string, modversion CRCs and code/data sections.

### `[@ANCHOR: aredn_jool:compare_modabi_find]` `find`
Maps module file names to paths under a directory.

### `[@ANCHOR: aredn_jool:compare_modabi_main]` `main`
Compares every same-named module of the release and the build; exit 0 only if enough were compared and all are identical.
