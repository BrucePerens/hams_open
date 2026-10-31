/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* Copyright (C) 2026 Bruce Perens and the hams.com contributors. */
#ifndef FX2_TIMER_H
#define FX2_TIMER_H

#include "fx2_regs.h"

/*
 * Timers (TRM chapter 14), all clocked CLKOUT/12 = 4 MHz at the 48 MHz CPU clock, so one count
 * is 250 ns:
 *   Timer 2  16-bit auto-reload, overflows every 1 ms: the millisecond tick (polled, no interrupt).
 *   Timer 1  16-bit one-shot used for the blocking delay_ms() of start-up code.
 *   Timer 0  16-bit free-running stopwatch (wraps every 16.4 ms), used to measure the longest
 *            time between two passes of the main loop (the firmware's worst-case latency).
 */
#define TIMER_COUNTS_PER_MS 4000U

void timer_init(void);
u8 timer_poll(void);                    /* 1 once per millisecond: call every main-loop pass */
u32 timer_ms(void);                     /* milliseconds since timer_init, advanced by timer_poll */
void timer_stamp(u32 *ms, u16 *sub);    /* ms and 250 ns counts into the current ms, consistent */
void timer_delay_ms(u16 ms);            /* blocking, Timer 1 */
void timer_loop_mark(void);             /* call once per main-loop pass */
void timer_loop_reset(void);            /* forget the maximum (after blocking start-up code) */
u16 timer_loop_max(void);               /* longest pass so far, 250 ns counts (saturates at 0xFFFF) */

#endif
