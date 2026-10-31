/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * Copyright (C) 2026 Bruce Perens and the hams.com contributors.
 *
 * Station I/O test image (hams device type 0x0001, provisional). Only PA0 and PA1
 * (the two LEDs on the development board, via J1) may ever be driven; every other
 * port pin stays an input. No transmit-affecting function, no time-out and no
 * heartbeat exist in this image: it is a bring-up image for the shared core.
 */
#include "core/fx2_core.h"
#include "core/hams_usb.h"
#include "core/io_engine.h"
#include "core/fx2_timer.h"

#define FW_MAJOR 0
#define FW_MINOR 2
#define FW_PATCH 0
#define HW_ID_GEEETECH_MINI 1    /* board map id: Geeetech "Lcsoft Mini Board" variant */
#define DRIVABLE_A 0x03          /* PA0, PA1 */

/* GET_INFO record, protocol 0.1 (layout documented in README.md, "GET_INFO record"). */
const __code __at(ADDR_APP_INFO) u8 app_get_info[23] = {
    23,                                     /*  0 length of the whole record */
    HAMS_PROTO_MAJOR, HAMS_PROTO_MINOR,     /*  1, 2 */
    HAMS_DEVTYPE_STATION_IO & 0xFF, HAMS_DEVTYPE_STATION_IO >> 8, /* 3, 4 sub-ID */
    FW_MAJOR, FW_MINOR, FW_PATCH,           /*  5-7 firmware version */
    HW_ID_GEEETECH_MINI,                    /*  8 hardware / board map id */
    (u8)((HAMS_CAP_GPIO_CMD | HAMS_CAP_MSOS20 | HAMS_CAP_WEBUSB | HAMS_CAP_SAFE_ON_RESET
          | HAMS_CAP_HW_TIMEOUT | HAMS_CAP_HEARTBEAT) & 0xFF),
    0, 0, 0,                                /*  9-12 capability flags, little endian */
    HAMS_SAFE_PER_MAP,                      /* 13 safe-state kind */
    IO_DEFAULT_MAX_ON_MS & 0xFF, IO_DEFAULT_MAX_ON_MS >> 8,       /* 14, 15 default max on-time ms */
    IO_DEFAULT_HEARTBEAT_MS & 0xFF, IO_DEFAULT_HEARTBEAT_MS >> 8, /* 16, 17 default heartbeat lapse ms */
    0x07,                                   /* 18 ports exposed: bit0 A, bit1 B, bit2 D */
    DRIVABLE_A,                             /* 19 port A pins the firmware may drive */
    1,                                      /* 20 endpoint count */
    0x00, HAMS_EPROLE_CONTROL               /* 21, 22 EP0: control / vendor commands */
};


const __code __at(ADDR_APP_PRODUCT) u8 app_str_product[70] = {
    70, 3,
    'H',0,'a',0,'m',0,'s',0,' ',0,'S',0,'t',0,'a',0,'t',0,'i',0,'o',0,'n',0,' ',0,'I',0,'/',0,
    'O',0,' ',0,'(',0,'F',0,'X',0,'2',0,'L',0,'P',0,',',0,' ',0,'t',0,'e',0,'s',0,'t',0,' ',0,
    'P',0,'I',0,'D',0,')',0
};

/* Board table: this board exposes all of PA, PB and PD, but only PA0 and PA1 (the two LEDs) may be
 * outputs, and they are channels 0 and 1. Pin id = port << 4 | bit. */
const u8 board_chan_pin[IO_NCH] = { 0, 1 };

u8 board_pin_channel(u8 pin)
{
    return (pin < IO_NCH) ? pin : IO_NO_CHAN;
}

static __xdata u8 g_reply[48];

static void release_outputs(void)
{
    io_release_all(IO_REL_USB_RESET);
}

void app_init(void)
{
    IFCONFIG = IFCONFIG_PORTS; SYNCDELAY();   /* B and D are plain I/O, not FIFO pins */
    PORTACFG = 0;                             /* PA7/FLAGD and PA3/WU2 etc: plain I/O */
    OEA = 0x00; OEB = 0x00; OED = 0x00;       /* everything an input (TRM 13.2) */
    io_init();
}

void app_usb_reset(void)
{
    release_outputs();
}

void app_usb_suspend(void)
{
    io_release_all(IO_REL_USB_SUSPEND);
}

void app_tick(void)
{
    io_tick();
}

u8 app_vendor_request(u8 bm, u8 req, u16 value, u16 index, u16 length)
{
    u8 st[4];
    (void)length;
    if (bm == 0x40 && req >= HAMS_REQ_MAP_BEGIN && req <= HAMS_REQ_KEY) {
        u8 ok = 0;
        switch (req) {
        case HAMS_REQ_MAP_BEGIN: ok = io_map_begin(); break;
        case HAMS_REQ_MAP_PIN: ok = io_map_pin((u8)value, (u8)(value >> 8), (u8)index); break;
        case HAMS_REQ_MAP_PARAM: ok = io_map_param((u8)value, (u8)(value >> 8), index); break;
        case HAMS_REQ_MAP_HEARTBEAT: ok = io_map_heartbeat(index); break;
        case HAMS_REQ_MAP_COMMIT: ok = io_map_commit(); break;
        case HAMS_REQ_KEY: ok = io_key((u8)value, (u8)index); break;
        default: break;
        }
        if (!ok) {
            return 0;
        }
        core_ep0_ack();
        return 1;
    }
    if (bm == 0xC0 && req == HAMS_REQ_GET_STATUS) {
        core_ep0_reply(g_reply, io_fill_status(g_reply));
        return 1;
    }
    if (bm == 0xC0 && req == HAMS_REQ_HEARTBEAT) {
        (void)io_heartbeat(value, (u8)index);
        core_ep0_reply(g_reply, io_fill_status(g_reply));
        return 1;
    }
    if (bm == 0xC0 && req == HAMS_REQ_GET_EVENTS) {
        core_ep0_reply(g_reply, io_fill_events(g_reply));
        return 1;
    }
    if (bm == 0xC0 && req == HAMS_REQ_GET_EDGES) {
        core_ep0_reply(g_reply, io_fill_edges(g_reply));
        return 1;
    }
    if (req == HAMS_REQ_GPIO_SET && bm == 0x40 && io_map_state() != IO_MAP_VALID) {
        u8 oe = (u8)value;
        u8 lvl = (u8)index;
        if ((oe & (u8)~DRIVABLE_A) || (lvl & (u8)~DRIVABLE_A) || (value >> 8) || (index >> 8)) {
            return 0;
        }
        IOA = lvl;          /* latch first, then enable, so a pin never glitches */
        OEA = oe;
        core_ep0_ack();
        return 1;
    }
    if (req == HAMS_REQ_GPIO_GET && bm == 0xC0) {
        st[0] = OEA;
        st[1] = IOA;
        st[2] = IOB;
        st[3] = IOD;
        core_ep0_reply(st, 4);
        return 1;
    }
    return 0;
}

void main(void)
{
    core_run();
}
