# grub_large_console

The package `grub-large-console` makes GRUB's menu and console readable on a high-resolution screen: about
**80 columns by 24 rows** on the machine's own panel. GRUB draws text with a small fixed font, so on a 3840x2400 laptop
screen the menu is microscopic.

On installation (and with `dpkg-reconfigure grub-large-console`) the script
`/usr/lib/grub-large-console/configure-grub-font`:

1. reads the native mode of the first connected display from `/sys/class/drm/*/modes`;
2. sizes DejaVu Sans Mono so that 80 columns fill the width (and at least 24 rows fit):
   3840x2400 gives 79 px, 2880x1800 59 px, 1920x1080 38 px, 1366x768 27 px, 1024x768 21 px;
3. generates `/boot/grub/fonts/convertible-80x24.pf2` with `grub-mkfont`;
4. writes `/etc/default/grub.d/90-convertible-large-console.cfg` (`GRUB_FONT`, `GRUB_GFXMODE` set to the native mode,
   `GRUB_GFXPAYLOAD_LINUX=keep`; drop-ins are read after `/etc/default/grub`, so they win over a hand-set `GRUB_FONT`);
5. runs `update-grub`.

Removing the package deletes the drop-in and the font and runs `update-grub` again. In a chroot or container with no
display it does nothing. `configure-grub-font --dry-run WxH` prints what it would write for a screen of that size.

The three device packages (`panasonic-cf19-support`, `panasonic-fz-g1-support`, `hp-spectre-x360-16-support`)
depend on this package.

## Verified and not verified

Verified (2026-10-29): the sizing for the screens above; the font generates; real GRUB 2.12 (UEFI build) shown at
1920x1080 under QEMU with OVMF using the 38 px font, a clearly readable menu about 76 columns wide.

Not verified: a boot on a real machine with this package installed (it was not installed on any of the three
devices, because that needs a reboot someone can look at); BIOS (legacy) boot, as on the CF-19; machines with several
displays (the first connected one is used).

Licence: GPL-3.0-or-later.
