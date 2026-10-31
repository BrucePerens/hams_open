/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* Copyright (C) 2026 Bruce Perens and the hams.com contributors. */
#include "fx2_timer.h"

#define T2_RELOAD ((u16)(65536UL - TIMER_COUNTS_PER_MS))

static volatile u32 g_ms;
static u16 g_last_mark;
static u16 g_loop_max;

void timer_init(void)
{
    CKCON &= (u8)~(CKCON_T2M | CKCON_T1M | CKCON_T0M);   /* all three at CLKOUT/12 */
    TMOD = (u8)(TMOD_T0_MODE1 | TMOD_T1_MODE1);
    TH0 = 0; TL0 = 0;
    TCON |= TCON_TR0;                                    /* Timer 0 free-runs */
    RCAP2H = (u8)(T2_RELOAD >> 8);
    RCAP2L = (u8)(T2_RELOAD & 0xFF);
    TH2 = (u8)(T2_RELOAD >> 8);
    TL2 = (u8)(T2_RELOAD & 0xFF);
    T2CON = T2CON_TR2;                                   /* 16-bit auto-reload, run */
    g_ms = 0;
    g_loop_max = 0;
    g_last_mark = ((u16)TH0 << 8) | TL0;
}

u8 timer_poll(void)
{
    if (T2CON & T2CON_TF2) {
        T2CON &= (u8)~T2CON_TF2;
        g_ms++;
        return 1;
    }
    return 0;
}

u32 timer_ms(void)
{
    return g_ms;
}

static u16 t2_count(void)
{
    u8 h, l;
    do {                      /* re-read when TL2 carried into TH2 between the two reads */
        h = TH2;
        l = TL2;
    } while (h != TH2);
    return ((u16)h << 8) | l;
}

void timer_stamp(u32 *ms, u16 *sub)
{
    u16 c;
    u8 pending;
    for (;;) {
        u8 before = T2CON & T2CON_TF2;
        c = t2_count();
        pending = T2CON & T2CON_TF2;
        if (!pending || before) {     /* the overflow was already pending, or none happened */
            break;
        }
    }
    *ms = g_ms + (pending ? 1UL : 0UL);
    *sub = (u16)(c - T2_RELOAD);
}

void timer_delay_ms(u16 ms)
{
    while (ms--) {
        TCON &= (u8)~(TCON_TR1 | TCON_TF1);
        TH1 = (u8)(T2_RELOAD >> 8);
        TL1 = (u8)(T2_RELOAD & 0xFF);
        TCON |= TCON_TR1;
        while (!(TCON & TCON_TF1)) { }
    }
    TCON &= (u8)~(TCON_TR1 | TCON_TF1);
}

void timer_loop_mark(void)
{
    u8 h, l;
    u16 now, d;
    do {
        h = TH0;
        l = TL0;
    } while (h != TH0);
    now = ((u16)h << 8) | l;
    d = (u16)(now - g_last_mark);
    g_last_mark = now;
    if (d > g_loop_max) {
        g_loop_max = d;
    }
}

u16 timer_loop_max(void)
{
    return g_loop_max;
}

void timer_loop_reset(void)
{
    g_loop_max = 0;
    timer_loop_mark();       /* a pass is measured between two marks: restart from here */
    g_loop_max = 0;
}
