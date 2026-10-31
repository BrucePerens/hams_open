/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* Copyright (C) 2026 Bruce Perens and the hams.com contributors. */
#ifndef IO_ENGINE_H
#define IO_ENGINE_H

#include "fx2_regs.h"

/*
 * GPIO function-map engine: function map, ptt/keyline/power channels with firmware-enforced
 * time-outs and sequencing, host heartbeat supervision, release and edge logs.
 * The image supplies the board table (which pins may be outputs); see station_io.c.
 */

/* Board table, provided by the image: channel index (0 .. IO_NCH-1) for a pin id, or 0xFF when the
 * board map does not allow that pin as an output. Pin id = port << 4 | bit, port 0 = A, 1 = B, 2 = D. */
#define IO_NCH 2
#define IO_NO_CHAN 0xFF
extern u8 board_pin_channel(u8 pin);
extern const u8 board_chan_pin[IO_NCH];

/* Functions. */
#define IO_FUNC_NONE 0
#define IO_FUNC_PTT 1
#define IO_FUNC_KEYLINE 2
#define IO_FUNC_POWER 3

/* MAP_PIN flags. */
#define IO_FLAG_SAFE_MASK 0x03      /* 0 = driven low, 1 = driven high, 2 = released (input) */
#define IO_FLAG_ACTIVE_HIGH 0x04
#define IO_FLAG_SAFE_DEFINED 0x08

/* MAP_PARAM ids. */
#define IO_PARAM_MAX_ON_MS 1
#define IO_PARAM_MIN_OFF_MS 2
#define IO_PARAM_LEAD_MS 3
#define IO_PARAM_LAG_MS 4
#define IO_PARAM_PARTNER 5          /* pin id of the keyline a ptt channel sequences */

/* Defaults (reported in GET_INFO). */
#define IO_DEFAULT_MAX_ON_MS 5000
#define IO_DEFAULT_HEARTBEAT_MS 2000

/* Map errors (status byte). */
#define IO_ERR_NONE 0
#define IO_ERR_PIN_NOT_ALLOWED 1
#define IO_ERR_PIN_TWO_FUNCTIONS 2
#define IO_ERR_BAD_FUNCTION 3
#define IO_ERR_SAFE_UNDEFINED 4
#define IO_ERR_SAFE_IS_ACTIVE 5
#define IO_ERR_BAD_PARAM 6
#define IO_ERR_PARTNER 7
#define IO_ERR_NO_TIMEOUT 8
#define IO_ERR_NO_HEARTBEAT 9
#define IO_ERR_NOT_LOADING 10
#define IO_ERR_LEAD_LAG_NO_KEYLINE 11

/* Release reasons (status, release log). */
#define IO_REL_NONE 0
#define IO_REL_TIMEOUT 1
#define IO_REL_HB_LAPSE 2
#define IO_REL_HB_SEQUENCE 3
#define IO_REL_HB_MISMATCH 4
#define IO_REL_USB_RESET 5
#define IO_REL_USB_SUSPEND 6
#define IO_REL_HOST 7               /* the host asked for key-up */
#define IO_REL_MAP_CLEARED 8

/* Key-down refusals (status byte, request stalls). */
#define IO_REFUSE_NONE 0
#define IO_REFUSE_NO_MAP 1
#define IO_REFUSE_NO_HEARTBEAT 2
#define IO_REFUSE_MIN_OFF 3
#define IO_REFUSE_BUSY 4
#define IO_REFUSE_BAD_CHANNEL 5

#define IO_STATUS_LEN 23
#define IO_MAP_NONE 0
#define IO_MAP_LOADING 1
#define IO_MAP_VALID 2

void io_init(void);
u8 io_map_state(void);
/* Each map step returns 1 when accepted; on 0 the error code is in the status. */
u8 io_map_begin(void);
u8 io_map_pin(u8 pin, u8 func, u8 flags);
u8 io_map_param(u8 pin, u8 param, u16 value);
u8 io_map_heartbeat(u16 lapse_ms);
u8 io_map_commit(void);
u8 io_key(u8 pin, u8 down);                     /* 1 accepted, 0 refused */
u8 io_heartbeat(u16 seq, u8 commanded);          /* fills the status; always answers */
void io_release_all(u8 reason);
void io_tick(void);                              /* once per millisecond */
u8 io_fill_status(u8 *dst);                      /* IO_STATUS_LEN bytes */
u8 io_fill_events(u8 *dst);
u8 io_fill_edges(u8 *dst);

#endif
