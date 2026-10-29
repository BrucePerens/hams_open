# Keeping the sound server away from the VNWA's USB codecs

The DG8SAQ VNWA 3E has two PCM2900C USB sound codecs (USB `08bb:29c0`) inside it. They are its measurement receivers, not sound cards: the driver (`vnwa_open`) opens
them directly through ALSA. If the desktop sound server manages them, system sounds and speech can be played into the analyzer (this happened once), and the server can hold
a card open during a sweep ("device busy").

Added by bench_mcp / `vnwa_open`. Repository: https://github.com/BrucePerens/hams_open (bench_mcp: `daemons/bench_mcp`, driver: `daemons/vnwa_open`).

## Files (in this directory)

| File | Installed as | For |
|---|---|---|
| `wireplumber/51-vnwa-codecs.conf` | `/etc/wireplumber/wireplumber.conf.d/51-vnwa-codecs.conf` | PipeWire with WirePlumber 0.5: `device.disabled = true` for `08bb:29c0`. Broad (it hides any such dongle; see its CAVEAT). |
| `udev/60-vnwa-codecs.rules` | `/etc/udev/rules.d/60-vnwa-codecs.rules` | PulseAudio (`PULSE_IGNORE`) and PipeWire's ALSA monitor (`ACP_IGNORE`, not verified on a running desktop). Narrow: only codecs with a VNWA control chip (`20a0:4118`) beside them. |
| `udev/vnwa-is-codec` | `/usr/local/libexec/vnwa-is-codec` | the helper the udev rule calls (reads sysfs, changes nothing) |

`08bb:29c0` alone is the generic Texas Instruments PCM2900C id of many cheap USB sound dongles. The udev rule and `vnwa_open`'s checks (`reserved::is_vnwa_codec`)
therefore require the VNWA control chip as a sibling under the same hub; the WirePlumber 0.5 rule cannot express that and is broad.

## Install

    vnwa_open install-system-config            # dry run: prints every file with its content and every command
    vnwa_open install-system-config --apply    # writes under /etc and /usr/local/libexec (uses sudo -n), then `udevadm control --reload`
    vnwa_open install-system-config --check    # state of the three files and whether the sound server lists the codecs; changes nothing
    vnwa_open install-system-config --uninstall --apply

What it does not do, on purpose: it never restarts or reloads WirePlumber or PipeWire (that drops every audio client, the desktop shell included, and WirePlumber 0.5 has no
reload), never runs `udevadm trigger` (which would make the codecs vanish and reappear in the middle of a measurement), refuses while a sweep has a codec open (`--force` to
override), backs up any file it replaces as `<name>.vnwa-backup`, undoes its steps if one fails, and logs to `~/.local/state/vnwa_open/install.log`. The rules take effect at the next
login or boot, or after the unit is unplugged and plugged in. `--restart-audio` adds `systemctl --user restart wireplumber` (off by default; it briefly drops the desktop's audio).

To hide the codecs from a running PipeWire right now without touching other clients: `wpctl status` shows nothing for the codecs when they are hidden; if a card is listed,
`wpctl set-profile <card id> 0` switches it off (the id is in `bench_status` under `codecs_protection_detail`).

## Other sound server setups

* WirePlumber 0.4 (Lua configuration, Debian 12 and older): put this in `~/.config/wireplumber/main.lua.d/51-vnwa-codecs.lua` (or `/etc/wireplumber/main.lua.d/`):

      rule = {
        matches = { { { "device.vendor.id", "equals", "0x08bb" }, { "device.product.id", "equals", "0x29c0" } } },
        apply_properties = { ["device.disabled"] = true },
      }
      table.insert(alsa_monitor.rules, rule)

  (not tested here: this machine runs WirePlumber 0.5.8).
* Plain PulseAudio: the udev rule above (`PULSE_IGNORE=1`) is the supported way; nothing else is needed.
* No sound server (ALSA only): nothing to do.
