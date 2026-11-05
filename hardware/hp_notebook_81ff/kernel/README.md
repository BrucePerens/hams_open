# A kernel for the HP 81FF that keeps ASPM on everywhere but the wireless link

`0001-pci-quirk-rtl8188ee-no-aspm.patch` adds a PCI quirk for the Realtek RTL8188EE (`10ec:8179`): at boot and again after resume it
clears the ASPM control bits (L0s/L1) at both ends of that device's link. On this board the firmware's FADT tells Linux it may not
manage ASPM ("FADT indicates ASPM is unsupported, using BIOS configuration"), so the BIOS leaves L0s/L1 on, the kernel never touches
them, `pci_disable_link_state()` cannot help, and the link floods the log with correctable `RxErr` errors (about 2000 a minute after an
NVRAM reset on 2026-11-04). Clearing the bits on that one link stops the errors; the package's global `pcie_aspm=off` is then not needed.

The driver option `rtl8188ee aspm=0` (in this package's `modprobe.d/`) must stay: with its default the driver turns ASPM back on for
its own link when it enters power saving. On 2026-11-04 that undid a run-time fix after 25 quiet minutes and the error storm
(27,000 a minute) hung the machine.

## Built and verified on bruce17 (2026-11-04)

- Source: linux-7.3.1 from kernel.org, signature checked (`linux-7.3.1.tar.sign`, Greg Kroah-Hartman, key fingerprint
  `647F 2865 4894 E3BD 4571 99BE 38DB BDC8 6092 693E`); sha256 of the `.tar.xz`
  `fa595aad93734f829c963c2a90ef7c3673224fd7fdae7b49406e397cde2de231`.
- Config: Debian's own (`/boot/config-6.12.115+deb13.1-amd64`), `make olddefconfig`, with `SYSTEM_TRUSTED_KEYS` and
  `SYSTEM_REVOCATION_KEYS` emptied (Debian's signing certificates are not available), `DEBUG_INFO_NONE`, `LOCALVERSION=-hams1`.
  Secure Boot is off on this machine, so the unsigned kernel boots.
- Build: `make -j4 bindeb-pkg` (needs `build-essential bc bison flex libssl-dev libelf-dev libdw-dev dwarves rsync cpio kmod
  debhelper`); about four hours on the i7-7500U.
- Result: booted without `pcie_aspm=off`: the quirk logs "ASPM L0s/L1 disabled on this link (RxErr flood)", both ends read
  `ASPM Disabled`, 0 PCIe errors over a 32-minute soak with network traffic and after a reboot through the default entry; wifi works.

## Boot entries

`/etc/grub.d/09_hams_kernel` is a hand-written GRUB entry for 7.3.1-hams1 **without** `pcie_aspm=off`, and it is the default
(`GRUB_DEFAULT=saved`, `grub-set-default hams-7.3.1`). The generated entries (stock 6.12 and 7.3.1) keep this package's
`pcie_aspm=off`, so choosing the stock kernel from the menu is still safe.
