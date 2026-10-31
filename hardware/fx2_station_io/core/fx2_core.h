/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* Copyright (C) 2026 Bruce Perens and the hams.com contributors. */
#ifndef FX2_CORE_H
#define FX2_CORE_H

#include "fx2_regs.h"

/* Provided by the application image. */
extern void app_init(void);
extern void app_usb_reset(void);
extern void app_usb_suspend(void);   /* 3 ms without bus activity (TRM 4.4.2.3) */
extern void app_tick(void);          /* once per millisecond, from the main loop */
/* Return 1 if handled (reply sent or acknowledged), 0 to stall. */
extern u8 app_vendor_request(u8 bm_request_type, u8 request, u16 value, u16 index, u16 length);
/* The image supplies a GET_INFO record at ADDR_APP_INFO (byte 0 = total length) and its product
 * string descriptor at ADDR_APP_PRODUCT; fixed addresses keep both word aligned for SUDPTR. */
#define ADDR_APP_INFO 0x2300
#define ADDR_APP_PRODUCT 0x2200
#define APP_GET_INFO ((const __code u8 *)ADDR_APP_INFO)
#define APP_STR_PRODUCT ((const __code u8 *)ADDR_APP_PRODUCT)

/* Core services. */
void core_ep0_reply(const u8 *src, u8 n);          /* up to 64 bytes from RAM */
void core_ep0_send_code(const __code u8 *p, u16 n); /* from code RAM via SUDPTR */
void core_ep0_ack(void);
void core_ep0_stall(void);
void core_run(void);   /* never returns */

#endif
