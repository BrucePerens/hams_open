/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * Copyright (C) 2026 Bruce Perens and the hams.com contributors.
 *
 * Constants of the hams USB device protocol, version 0.1
 * (hams_com docs/proposals/HAMS_USB_DEVICE_PROTOCOL.md section 4). The GET_INFO
 * record layout is documented in ../README.md and mirrored in host/hams_fx2.py.
 */
#ifndef HAMS_USB_H
#define HAMS_USB_H

#define HAMS_PROTO_MAJOR 0
#define HAMS_PROTO_MINOR 2

/* Vendor requests (bmRequestType type = vendor). 0xA0-0xAF are reserved by Cypress (TRM 3.8). */
#define HAMS_REQ_GET_INFO 0x01   /* IN, device: the self-describing record */
#define HAMS_REQ_GPIO_SET 0x02   /* OUT, no data: wValueL = output-enable mask, wIndexL = levels */
#define HAMS_REQ_GPIO_GET 0x03   /* IN, 4 bytes: OEA, IOA, IOB, IOD (pin states) */
#define HAMS_REQ_GET_TICK 0x04   /* IN, 8 bytes: ms (u32), 250 ns counts into the ms (u16), longest loop pass (u16) */
#define HAMS_REQ_GET_STATUS 0x05 /* IN, 23 bytes: the status record (README, "Status record") */
#define HAMS_REQ_HEARTBEAT  0x06 /* IN, wValue = sequence, wIndexL = commanded state; answers the status */
#define HAMS_REQ_GET_EVENTS 0x07 /* IN: release log, newest first */
#define HAMS_REQ_GET_EDGES  0x08 /* IN: output edge log with timestamps, newest first */
#define HAMS_REQ_MAP_BEGIN  0x10 /* OUT, no data: start a function map (releases everything) */
#define HAMS_REQ_MAP_PIN    0x11 /* OUT: wValueL = pin id, wValueH = function, wIndexL = flags */
#define HAMS_REQ_MAP_PARAM  0x12 /* OUT: wValueL = pin id, wValueH = parameter id, wIndex = value */
#define HAMS_REQ_MAP_HEARTBEAT 0x13 /* OUT: wIndex = heartbeat lapse in ms */
#define HAMS_REQ_MAP_COMMIT 0x14 /* OUT: validate and activate the map (stalls on error) */
#define HAMS_REQ_KEY        0x15 /* OUT: wValueL = pin id, wIndexL = 1 key down, 0 key up */
#define HAMS_REQ_MSOS20   0x20   /* Microsoft OS 2.0 descriptor set, wIndex 7 */
#define HAMS_REQ_WEBUSB   0x21   /* WebUSB vendor code (no landing page offered) */

/* Device types (the sub-ID). Provisional: no registry exists yet. */
#define HAMS_DEVTYPE_STATION_IO 0x0001

/* Capability flags (GET_INFO, 32 bits little-endian). */
#define HAMS_CAP_GPIO_CMD   0x00000001UL
#define HAMS_CAP_MSOS20     0x00000002UL
#define HAMS_CAP_WEBUSB     0x00000004UL
#define HAMS_CAP_HW_TIMEOUT 0x00000008UL  /* hardware transmit time-out  */
#define HAMS_CAP_HEARTBEAT  0x00000010UL  /* heartbeat supervision  */
#define HAMS_CAP_SAFE_ON_RESET 0x00000020UL /* releases outputs on USB bus reset */

/* Safe-state kinds. */
#define HAMS_SAFE_RELEASE_ALL 0   /* every output released to input (high impedance) */
#define HAMS_SAFE_PER_MAP 1       /* each channel goes to the safe state its map entry names */

/* Endpoint roles. */
#define HAMS_EPROLE_CONTROL 1

#endif
