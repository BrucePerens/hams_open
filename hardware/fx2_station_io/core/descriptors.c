/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * Copyright (C) 2026 Bruce Perens and the hams.com contributors.
 *
 * USB descriptors for the shared core. Written from the USB 2.0 specification
 * chapter 9 (descriptor layouts), the TRM section 2.3.4 / Appendix B (the FX2LP
 * serves descriptors from RAM through SUDPTR, which must be word aligned), the
 * WebUSB specification (BOS platform capability) and the Microsoft OS 2.0
 * Descriptors specification (BOS platform capability and descriptor set).
 *
 * Each table sits at a fixed even code-RAM address so SUDPTRH:L (TRM 15.12.26)
 * can point straight at it; the linker reports an error if code grows into them.
 */
#include "fx2_regs.h"
#include "hams_usb.h"

/* Test PID 0x0001 under pid.codes vendor 0x1209 (development only, never shipped). */
#define VID_LO 0x09
#define VID_HI 0x12
#define PID_LO 0x01
#define PID_HI 0x00

/* Device descriptor: USB 2.1 (needed for BOS), class at interface level, EP0 max 64. */
const __code __at(0x2000) u8 desc_device[18] = {
    18, 1, 0x10, 0x02, 0x00, 0x00, 0x00, 64,
    VID_LO, VID_HI, PID_LO, PID_HI, 0x01, 0x00,   /* bcdDevice 0.1 */
    1, 2, 3, 1
};

const __code __at(0x2020) u8 desc_qualifier[10] = {
    10, 6, 0x10, 0x02, 0x00, 0x00, 0x00, 64, 1, 0
};

/* One configuration, one vendor-specific interface (class 0xFF, subclass 1 = switch/I-O),
 * no endpoints beyond EP0, bus powered, 100 mA. */

const __code __at(0x2030) u8 desc_config[18] = {
    9, 2, 18, 0, 1, 1, 0, 0x80, 50,
    9, 4, 0, 0, 0, 0xFF, 0x01, 0x00, 0
};

const __code __at(0x2050) u8 desc_other_speed[18] = {
    9, 7, 18, 0, 1, 1, 0, 0x80, 50,
    9, 4, 0, 0, 0, 0xFF, 0x01, 0x00, 0
};

const __code __at(0x2070) u8 desc_langid[4] = { 4, 3, 0x09, 0x04 };

const __code __at(0x2080) u8 str_manufacturer[18] = {
    18, 3,
    'h',0,'a',0,'m',0,'s',0,'.',0,'c',0,'o',0,'m',0
};

/* Placeholder serial: the FX2 has no unique chip ID; a per-board serial comes from the
 * (optional, not yet implemented) EEPROM identity step. */
const __code __at(0x20A0) u8 str_serial[38] = {
    38, 3,
    'H',0,'A',0,'M',0,'S',0,'-',0,'F',0,'X',0,'2',0,'-',0,'T',0,'E',0,'S',0,'T',0,'-',0,
    '0',0,'0',0,'0',0,'1',0
};

/* BOS (57 bytes): header, WebUSB platform capability (24), MS OS 2.0 platform capability (28). */
const __code __at(0x20D0) u8 desc_bos[57] = {
    5, 0x0F, 57, 0, 2,
    /* WebUSB: UUID {3408b638-09a9-47a0-8bfd-a0768815b665}, bcdVersion 1.0, vendor code, no landing page */
    24, 0x10, 0x05, 0,
    0x38,0xB6,0x08,0x34, 0xA9,0x09, 0xA0,0x47, 0x8B,0xFD, 0xA0,0x76,0x88,0x15,0xB6,0x65,
    0x00, 0x01, HAMS_REQ_WEBUSB, 0,
    /* Microsoft OS 2.0: UUID {D8DD60DF-4589-4CC7-9CD2-659D9E648A9F}, Windows 8.1 (0x06030000),
     * descriptor set length 162, vendor code, no alternate enumeration */
    28, 0x10, 0x05, 0,
    0xDF,0x60,0xDD,0xD8, 0x89,0x45, 0xC7,0x4C, 0x9C,0xD2, 0x65,0x9D,0x9E,0x64,0x8A,0x9F,
    0x00,0x00,0x03,0x06, 162, 0, HAMS_REQ_MSOS20, 0
};

/* MS OS 2.0 descriptor set (162 bytes): set header, compatible ID "WINUSB", and a
 * DeviceInterfaceGUIDs registry property so WinUSB user-mode code can find the device. */
const __code __at(0x2110) u8 desc_msos20[162] = {
    10, 0, 0x00, 0x00, 0x03, 0x06, 162, 0,          /* set header: wLength, type 0, version, total */
    20, 0, 3, 0, 'W','I','N','U','S','B',0,0, 0,0,0,0,0,0,0,0, /* compatible ID */
    132, 0, 4, 0, 7, 0, 42, 0,                       /* registry property, REG_MULTI_SZ, name length */
    'D',0,'e',0,'v',0,'i',0,'c',0,'e',0,'I',0,'n',0,'t',0,'e',0,'r',0,'f',0,'a',0,'c',0,'e',0,'G',0,'U',0,'I',0,'D',0,'s',0, 0,0,
    80, 0,                                           /* data length: 38 chars + NUL + NUL, UTF-16 */
    '{',0,'3',0,'0',0,'8',0,'f',0,'9',0,'0',0,'1',0,'7',0,'-',0,'5',0,'3',0,'a',0,'2',0,'-',0,'4',0,'f',0,'9',0,'8',0,'-',0,'8',0,'2',0,'c',0,'8',0,'-',0,'a',0,'a',0,'3',0,'a',0,'7',0,'9',0,'4',0,'7',0,'f',0,'0',0,'b',0,'8',0,'}',0, 0,0, 0,0
};
