/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * Copyright (C) 2026 Bruce Perens and the hams.com contributors.
 *
 * Shared FX2LP core: clock, ReNumeration (TRM 3.9), endpoint zero (TRM chapter 2),
 * standard requests, Microsoft OS 2.0 / WebUSB vendor requests, vendor dispatch.
 * USB events are polled from USBIRQ in one loop; no interrupts are used, so there
 * is no vector table beyond the reset vector.
 */
#include "fx2_core.h"
#include "hams_usb.h"
#include "fx2_timer.h"

extern const __code u8 desc_device[18];
extern const __code u8 desc_qualifier[10];
extern const __code u8 desc_config[18];
extern const __code u8 desc_other_speed[18];
extern const __code u8 desc_langid[4];
extern const __code u8 str_manufacturer[18];
extern const __code u8 str_serial[38];
extern const __code u8 desc_bos[57];
extern const __code u8 desc_msos20[162];

static u8 g_config;      /* current configuration value (0 or 1) */

void core_ep0_ack(void)
{
    EP0CS = EP0CS_HSNAK;          /* write 1 to clear HSNAK: ACK the status stage (TRM 2.2) */
}

void core_ep0_stall(void)
{
    EP0CS = EP0CS_STALL | EP0CS_HSNAK;   /* stall needs both bits (TRM 2.2) */
}

/* Small replies: fill EP0BUF, load the byte count, then release HSNAK (TRM 2.3.1). */
void core_ep0_reply(const u8 *src, u8 n)
{
    u8 i;
    SUDPTRCTL = 1;              /* auto mode: EP0BCL then arms the buffer (TRM 15.12.27) */
    core_ep0_ack();
    for (i = 0; i < n; i++) {
        EP0BUF[i] = src[i];
    }
    EP0BCH = 0;
    SYNCDELAY();
    EP0BCL = n;
    SYNCDELAY();
}

/* Longer replies from code RAM: manual SUDPTR mode (TRM 15.12.27): length in EP0BCH:L first,
 * then loading SUDPTRL starts the transfer. The SIE sends min(wLength, n). */
void core_ep0_send_code(const __code u8 *p, u16 n)
{
    core_ep0_ack();
    SUDPTRCTL = 0;
    EP0BCH = (u8)(n >> 8);
    SYNCDELAY();
    EP0BCL = (u8)(n & 0xFF);
    SYNCDELAY();
    SUDPTRH = (u8)(((u16)p) >> 8);
    SUDPTRL = (u8)(((u16)p) & 0xFF);
}

static void reply_status(void)
{
    static const u8 zero2[2] = { 0, 0 };   /* bus powered, no remote wakeup, not halted */
    core_ep0_reply(zero2, 2);
}

static void get_descriptor(u8 type, u8 index, u16 length)
{
    (void)length;
    switch (type) {
    case 1:
        core_ep0_send_code(desc_device, sizeof desc_device);
        return;
    case 2:
        if (index == 0) {
            core_ep0_send_code(desc_config, sizeof desc_config);
            return;
        }
        break;
    case 3:
        if (index == 0) { core_ep0_send_code(desc_langid, sizeof desc_langid); return; }
        if (index == 1) { core_ep0_send_code(str_manufacturer, str_manufacturer[0]); return; }
        if (index == 2) { core_ep0_send_code(APP_STR_PRODUCT, APP_STR_PRODUCT[0]); return; }
        if (index == 3) { core_ep0_send_code(str_serial, str_serial[0]); return; }
        break;
    case 6:
        core_ep0_send_code(desc_qualifier, sizeof desc_qualifier);
        return;
    case 7:
        if (index == 0) {
            core_ep0_send_code(desc_other_speed, sizeof desc_other_speed);
            return;
        }
        break;
    case 15:
        core_ep0_send_code(desc_bos, sizeof desc_bos);
        return;
    default:
        break;
    }
    core_ep0_stall();
}

static void standard_request(u8 bm, u8 req, u16 value, u16 index, u16 length)
{
    switch (req) {
    case 0x00:                           /* GET_STATUS */
        if ((bm & 0x1F) == 2 && (index & 0x7F) != 0) { core_ep0_stall(); return; }
        reply_status();
        return;
    case 0x01:                           /* CLEAR_FEATURE: nothing is ever set */
        core_ep0_ack();
        return;
    case 0x06:                           /* GET_DESCRIPTOR */
        if (bm != 0x80) { core_ep0_stall(); return; }
        get_descriptor((u8)(value >> 8), (u8)value, length);
        return;
    case 0x08:                           /* GET_CONFIGURATION */
        core_ep0_reply(&g_config, 1);
        return;
    case 0x09:                           /* SET_CONFIGURATION */
        if ((value & 0xFF) > 1) { core_ep0_stall(); return; }
        g_config = (u8)value;
        core_ep0_ack();
        return;
    case 0x0A: {                         /* GET_INTERFACE: only alternate setting 0 exists */
        static const u8 alt0 = 0;
        core_ep0_reply(&alt0, 1);
        return;
    }
    case 0x0B:                           /* SET_INTERFACE */
        if (value == 0) { core_ep0_ack(); } else { core_ep0_stall(); }
        return;
    default:                             /* SET_FEATURE, SET_DESCRIPTOR, SYNCH_FRAME, reserved */
        core_ep0_stall();
        return;
    }
}

static void vendor_request(u8 bm, u8 req, u16 value, u16 index, u16 length)
{
    if (req == HAMS_REQ_GET_INFO && bm == 0xC0) {
        core_ep0_send_code(APP_GET_INFO, APP_GET_INFO[0]);
        return;
    }
    if (req == HAMS_REQ_GET_TICK && bm == 0xC0) {
        u8 r[8];
        u32 ms;
        u16 sub, mx;
        timer_stamp(&ms, &sub);
        mx = timer_loop_max();
        r[0] = (u8)ms; r[1] = (u8)(ms >> 8); r[2] = (u8)(ms >> 16); r[3] = (u8)(ms >> 24);
        r[4] = (u8)sub; r[5] = (u8)(sub >> 8);
        r[6] = (u8)mx; r[7] = (u8)(mx >> 8);
        core_ep0_reply(r, 8);
        return;
    }
    if (req == HAMS_REQ_MSOS20 && bm == 0xC0 && index == 7) {
        core_ep0_send_code(desc_msos20, sizeof desc_msos20);
        return;
    }
    if (!app_vendor_request(bm, req, value, index, length)) {
        core_ep0_stall();
    }
}

static void handle_setup(void)
{
    u8 bm = SETUPDAT[0];
    u8 req = SETUPDAT[1];
    u16 value = (u16)SETUPDAT[2] | ((u16)SETUPDAT[3] << 8);
    u16 index = (u16)SETUPDAT[4] | ((u16)SETUPDAT[5] << 8);
    u16 length = (u16)SETUPDAT[6] | ((u16)SETUPDAT[7] << 8);

    switch (bm & 0x60) {
    case 0x00:
        standard_request(bm, req, value, index, length);
        break;
    case 0x40:
        vendor_request(bm, req, value, index, length);
        break;
    default:
        core_ep0_stall();
        break;
    }
}

/* ReNumerate (TRM 3.9): drop off the bus, take over endpoint zero, come back. */
static void renumerate(void)
{
    USBCS |= USBCS_DISCON;
    USBCS |= USBCS_RENUM;
    timer_delay_ms(500);
    USBIRQ = 0xFF;                       /* clear stale request flags (write 1 to clear) */
    USBCS &= (u8)~USBCS_DISCON;
}

void core_run(void)
{
    /* 48 MHz CPU clock, CLKOUT pin released (TRM 15.5.1). */
    CPUCS = CPUCS_CLKSPD_48MHZ;
    SYNCDELAY();

    /* This image uses no endpoint but EP0: invalidate the bulk endpoints (TRM 15.6.1). */
    EP1OUTCFG = 0; SYNCDELAY();
    EP1INCFG = 0;  SYNCDELAY();
    EP2CFG = 0;    SYNCDELAY();
    EP4CFG = 0;    SYNCDELAY();
    EP6CFG = 0;    SYNCDELAY();
    EP8CFG = 0;    SYNCDELAY();

    timer_init();
    app_init();
    renumerate();
    timer_loop_reset();

    for (;;) {
        u8 irq = USBIRQ;
        timer_loop_mark();
        if (timer_poll()) {
            app_tick();
        }
        if (irq & USBIRQ_SUSP) {
            USBIRQ = USBIRQ_SUSP;
            app_usb_suspend();
        }
        if (irq & USBIRQ_URES) {
            USBIRQ = USBIRQ_URES;
            g_config = 0;
            app_usb_reset();
        }
        if (irq & USBIRQ_SUDAV) {
            USBIRQ = USBIRQ_SUDAV;
            handle_setup();
        }
    }
}
