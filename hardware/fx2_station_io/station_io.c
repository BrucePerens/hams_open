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

#define FW_MAJOR 0
#define FW_MINOR 1
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
    (u8)((HAMS_CAP_GPIO_CMD | HAMS_CAP_MSOS20 | HAMS_CAP_WEBUSB | HAMS_CAP_SAFE_ON_RESET) & 0xFF),
    0, 0, 0,                                /*  9-12 capability flags, little endian */
    HAMS_SAFE_RELEASE_ALL,                  /* 13 safe-state kind */
    0, 0,                                   /* 14, 15 hardware time-out ms (0 = none) */
    0, 0,                                   /* 16, 17 heartbeat period ms (0 = none) */
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

static void release_outputs(void)
{
    OEA = 0x00; OEB = 0x00; OED = 0x00;   /* safe state: everything an input (TRM 13.2) */
}

void app_init(void)
{
    IFCONFIG = IFCONFIG_PORTS; SYNCDELAY();   /* B and D are plain I/O, not FIFO pins */
    PORTACFG = 0;                             /* PA7/FLAGD and PA3/WU2 etc: plain I/O */
    release_outputs();
}

void app_usb_reset(void)
{
    release_outputs();
}

void app_usb_suspend(void)
{
    release_outputs();
}

void app_tick(void)
{
}

u8 app_vendor_request(u8 bm, u8 req, u16 value, u16 index, u16 length)
{
    u8 st[4];
    (void)length;
    if (req == HAMS_REQ_GPIO_SET && bm == 0x40) {
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
