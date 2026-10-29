---
title: Reserved devices
summary: Hardware that one driver owns and no other tool may open, such as the VNA's audio codecs.
see_also: [concepts/several-instruments, instruments/sound/overview]
verified: code only, 2026-10-29
---

# Reserved devices

Some hardware belongs to one driver while it is in use:

* **The VNWA's two USB audio codecs** (`08bb:29c0`, PCM2900C inside the unit) carry its measurement signals. The `bench_audio_*` tools list them as `reserved` and never open them, read their volume or play to them. The VNA driver finds them from their place in the USB tree, so the card numbers can change after a power cycle without harm.
* **The VNA itself** can be used by one call at a time; while a sweep runs, the vendor's program cannot use the codecs at the same time.
* **Protected USB devices**: the USB tools refuse to switch off, reset or unbind the device holding the root filesystem or a mount, keyboards, the bench's own serial adapters, the hubs on the path to those, and anything the configuration lists as critical. `bench_usb_protection` gives the verdict for a target.
* **Output of other jobs**: a sound output or input in use for a measurement should not be used for alerts at the same time.

If a tool says a device is reserved or protected, use a different device; do not look for a way around it.
