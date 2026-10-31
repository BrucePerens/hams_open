/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * Copyright (C) 2026 Bruce Perens and the hams.com contributors.
 *
 * GPIO function-map engine. Everything the transmit-safety design needs to hold when the host is
 * dead lives here, in the firmware: the maximum on-time, the minimum off time, the heartbeat
 * lapse, the release on bus reset and suspend. (hams_com docs/proposals/
 * RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md sections 4.2 and 4.4.)
 *
 * Channel states: IDLE, LEAD (keyline asserted, waiting for the lead time before ptt), ON,
 * LAG (ptt released, keyline held for the lag time). A fault releases every line at once.
 */
#include "io_engine.h"
#include "fx2_timer.h"

#define ST_IDLE 0
#define ST_LEAD 1
#define ST_ON 2
#define ST_LAG 3
#define LVL_UNKNOWN 0xFF
#define LVL_RELEASED 2
#define LOG_N 8

struct chan {
    u8 func, safe, active_high, partner, owner, lastlvl, st, off_valid;
    u16 max_on, min_off, lead, lag;
    u16 t_mark, t_phase, t_off;     /* milliseconds modulo 65536; every interval is below 65 s */
};

static __xdata struct chan g_ch[IO_NCH];
static __xdata u8 g_map_state, g_map_error, g_refusal, g_have_seq, g_alive, g_oos;
static __xdata u16 g_hb_timeout, g_last_seq;
static __xdata u16 g_hb_t;
static __xdata u8 g_shadow;                       /* port A latch; IOA reads the pins, never read-modify-write it */

static __xdata u8 g_rel_reason, g_rel_chan, g_rel_count;
static __xdata u8 g_ev_reason[LOG_N], g_ev_chan[LOG_N];
static __xdata u16 g_ev_ms[LOG_N];

static __xdata u8 g_edge_count, g_edge_pl[LOG_N];
static __xdata u16 g_edge_ms[LOG_N], g_edge_sub[LOG_N];

static u16 now16(void)
{
    return (u16)timer_ms();
}

static void log_edge(u8 c, u8 lvl)
{
    u32 ms;
    u16 sub;
    u8 i = g_edge_count & (LOG_N - 1);
    timer_stamp(&ms, &sub);
    g_edge_pl[i] = (u8)(c | (lvl << 4));
    g_edge_ms[i] = (u16)ms;
    g_edge_sub[i] = sub;
    g_edge_count++;
}

static void log_release(u8 reason, u8 c)
{
    u8 i = g_rel_count & (LOG_N - 1);
    g_ev_reason[i] = reason;
    g_ev_chan[i] = c;
    g_ev_ms[i] = now16();
    g_rel_count++;
    g_rel_reason = reason;
    g_rel_chan = c;
}

/* Drive a pin to a level (latch first, then output enable, so it never glitches). */
static void pin_level(u8 c, u8 lvl)
{
    u8 bit = (u8)(1 << board_chan_pin[c]);
    if (g_ch[c].lastlvl == lvl) {
        return;
    }
    if (lvl == LVL_RELEASED) {
        OEA &= (u8)~bit;
    } else {
        if (lvl) { g_shadow |= bit; } else { g_shadow &= (u8)~bit; }
        IOA = g_shadow;
        OEA |= bit;
    }
    g_ch[c].lastlvl = lvl;
    log_edge(c, lvl);
}

static void pin_active(u8 c, u8 active)
{
    pin_level(c, (u8)(active ? g_ch[c].active_high : !g_ch[c].active_high));
}

static void pin_safe(u8 c)
{
    pin_level(c, g_ch[c].safe == 2 ? LVL_RELEASED : g_ch[c].safe);
}

static u8 is_primary(u8 c)
{
    return g_ch[c].func == IO_FUNC_PTT || g_ch[c].func == IO_FUNC_POWER;
}

/* Logical state: primary channels that are keyed (LEAD or ON), with their keyline's bit. */
static u8 keyed_mask(void)
{
    u8 c, m = 0;
    for (c = 0; c < IO_NCH; c++) {
        if (is_primary(c) && (g_ch[c].st == ST_LEAD || g_ch[c].st == ST_ON)) {
            m |= (u8)(1 << c);
            if (g_ch[c].partner != IO_NO_CHAN) {
                m |= (u8)(1 << g_ch[c].partner);
            }
        }
    }
    return m;
}

static u8 active_mask(void)
{
    u8 c, m = 0;
    for (c = 0; c < IO_NCH; c++) {
        if (g_ch[c].func != IO_FUNC_NONE && g_ch[c].lastlvl != LVL_UNKNOWN
            && g_ch[c].lastlvl != LVL_RELEASED && (g_ch[c].lastlvl != 0) == (g_ch[c].active_high != 0)) {
            m |= (u8)(1 << c);
        }
    }
    return m;
}

/* Release one primary channel at once (primary line first, then its keyline). */
static void release_channel(u8 c, u8 reason)
{
    u8 was_keyed = (g_ch[c].st == ST_LEAD || g_ch[c].st == ST_ON);
    pin_safe(c);
    if (g_ch[c].partner != IO_NO_CHAN) {
        pin_safe(g_ch[c].partner);
    }
    g_ch[c].st = ST_IDLE;
    g_ch[c].t_off = now16();
    g_ch[c].off_valid = 1;
    if (was_keyed) {
        log_release(reason, c);
    }
}

void io_release_all(u8 reason)
{
    u8 c;
    for (c = 0; c < IO_NCH; c++) {
        if (is_primary(c) && g_ch[c].st != ST_IDLE) {
            release_channel(c, reason);
        }
    }
}

static void clear_map(void)
{
    u8 c;
    io_release_all(IO_REL_MAP_CLEARED);
    for (c = 0; c < IO_NCH; c++) {
        if (g_ch[c].lastlvl != LVL_UNKNOWN) {
            pin_level(c, LVL_RELEASED);        /* every mapped pin back to a plain input */
        }
        g_ch[c].func = IO_FUNC_NONE;
        g_ch[c].lastlvl = LVL_UNKNOWN;
        g_ch[c].st = ST_IDLE;
        g_ch[c].partner = IO_NO_CHAN;
        g_ch[c].owner = IO_NO_CHAN;
        g_ch[c].max_on = IO_DEFAULT_MAX_ON_MS;
        g_ch[c].min_off = 0;
        g_ch[c].lead = 0;
        g_ch[c].lag = 0;
        g_ch[c].off_valid = 0;
    }
    g_map_state = IO_MAP_NONE;
    g_alive = 0;
    g_have_seq = 0;
    g_oos = 0;
    g_hb_timeout = IO_DEFAULT_HEARTBEAT_MS;
}

void io_init(void)
{
    u8 c;
    g_shadow = 0xFF;
    g_map_error = IO_ERR_NONE;
    g_refusal = IO_REFUSE_NONE;
    g_rel_reason = IO_REL_NONE;
    g_rel_chan = 0;
    g_rel_count = 0;
    g_edge_count = 0;
    g_last_seq = 0;
    g_hb_t = 0;
    for (c = 0; c < IO_NCH; c++) {
        g_ch[c].lastlvl = LVL_UNKNOWN;
    }
    clear_map();
}

u8 io_map_state(void)
{
    return g_map_state;
}

static u8 fail(u8 code)
{
    g_map_error = code;
    return 0;
}

u8 io_map_begin(void)
{
    clear_map();
    g_map_error = IO_ERR_NONE;
    g_map_state = IO_MAP_LOADING;
    return 1;
}

u8 io_map_pin(u8 pin, u8 func, u8 flags)
{
    u8 c;
    if (g_map_state != IO_MAP_LOADING) {
        return fail(IO_ERR_NOT_LOADING);
    }
    c = board_pin_channel(pin);
    if (c == IO_NO_CHAN) {
        return fail(IO_ERR_PIN_NOT_ALLOWED);
    }
    if (func == IO_FUNC_NONE || func > IO_FUNC_POWER) {
        return fail(IO_ERR_BAD_FUNCTION);
    }
    if (g_ch[c].func != IO_FUNC_NONE) {
        return fail(IO_ERR_PIN_TWO_FUNCTIONS);
    }
    if (!(flags & IO_FLAG_SAFE_DEFINED) || (flags & IO_FLAG_SAFE_MASK) == 3) {
        return fail(IO_ERR_SAFE_UNDEFINED);
    }
    g_ch[c].safe = (u8)(flags & IO_FLAG_SAFE_MASK);
    g_ch[c].active_high = (flags & IO_FLAG_ACTIVE_HIGH) ? 1 : 0;
    /* a driven safe state must be the inactive level, never the active one */
    if (g_ch[c].safe != 2 && g_ch[c].safe == g_ch[c].active_high) {
        return fail(IO_ERR_SAFE_IS_ACTIVE);
    }
    g_ch[c].func = func;
    return 1;
}

u8 io_map_param(u8 pin, u8 param, u16 value)
{
    u8 c;
    if (g_map_state != IO_MAP_LOADING) {
        return fail(IO_ERR_NOT_LOADING);
    }
    c = board_pin_channel(pin);
    if (c == IO_NO_CHAN) {
        return fail(IO_ERR_PIN_NOT_ALLOWED);
    }
    if (g_ch[c].func != IO_FUNC_PTT) {
        return fail(IO_ERR_BAD_PARAM);
    }
    switch (param) {
    case IO_PARAM_MAX_ON_MS: g_ch[c].max_on = value; return 1;
    case IO_PARAM_MIN_OFF_MS: g_ch[c].min_off = value; return 1;
    case IO_PARAM_LEAD_MS: g_ch[c].lead = value; return 1;
    case IO_PARAM_LAG_MS: g_ch[c].lag = value; return 1;
    case IO_PARAM_PARTNER: {
        u8 k = board_pin_channel((u8)value);
        if (k == IO_NO_CHAN || k == c) {
            return fail(IO_ERR_PARTNER);
        }
        g_ch[c].partner = k;
        return 1;
    }
    default:
        return fail(IO_ERR_BAD_PARAM);
    }
}

u8 io_map_heartbeat(u16 lapse_ms)
{
    if (g_map_state != IO_MAP_LOADING) {
        return fail(IO_ERR_NOT_LOADING);
    }
    if (lapse_ms == 0) {
        return fail(IO_ERR_NO_HEARTBEAT);
    }
    g_hb_timeout = lapse_ms;
    return 1;
}

u8 io_map_commit(void)
{
    u8 c, k, any = 0;
    if (g_map_state != IO_MAP_LOADING) {
        return fail(IO_ERR_NOT_LOADING);
    }
    for (c = 0; c < IO_NCH; c++) {
        g_ch[c].owner = IO_NO_CHAN;
    }
    for (c = 0; c < IO_NCH; c++) {
        if (g_ch[c].func == IO_FUNC_NONE) {
            continue;
        }
        any = 1;
        if (g_ch[c].func == IO_FUNC_PTT) {
            if (g_ch[c].max_on == 0) {
                return fail(IO_ERR_NO_TIMEOUT);
            }
            k = g_ch[c].partner;
            if (k != IO_NO_CHAN) {
                if (g_ch[k].func != IO_FUNC_KEYLINE || g_ch[k].owner != IO_NO_CHAN) {
                    return fail(IO_ERR_PARTNER);
                }
                g_ch[k].owner = c;
            } else if (g_ch[c].lead || g_ch[c].lag) {
                return fail(IO_ERR_LEAD_LAG_NO_KEYLINE);
            }
        } else if (g_ch[c].func == IO_FUNC_POWER) {
            g_ch[c].partner = IO_NO_CHAN;
        }
    }
    for (c = 0; c < IO_NCH; c++) {
        if (g_ch[c].func == IO_FUNC_KEYLINE && g_ch[c].owner == IO_NO_CHAN) {
            return fail(IO_ERR_PARTNER);
        }
    }
    if (!any) {
        return fail(IO_ERR_BAD_FUNCTION);
    }
    g_map_state = IO_MAP_VALID;
    for (c = 0; c < IO_NCH; c++) {
        if (g_ch[c].func != IO_FUNC_NONE) {
            pin_safe(c);                  /* every mapped pin goes to its safe state now */
        }
    }
    return 1;
}

static u8 refuse(u8 why)
{
    g_refusal = why;
    return 0;
}

u8 io_key(u8 pin, u8 down)
{
    u8 c = board_pin_channel(pin);
    u16 now = now16();
    if (g_map_state != IO_MAP_VALID) {
        return refuse(IO_REFUSE_NO_MAP);
    }
    if (c == IO_NO_CHAN || !is_primary(c)) {
        return refuse(IO_REFUSE_BAD_CHANNEL);
    }
    if (!down) {
        if (g_ch[c].st == ST_LEAD || g_ch[c].st == ST_ON) {
            pin_safe(c);
            g_ch[c].t_off = now;
            g_ch[c].off_valid = 1;
            log_release(IO_REL_HOST, c);
            if (g_ch[c].partner != IO_NO_CHAN && g_ch[c].lag) {
                g_ch[c].st = ST_LAG;
                g_ch[c].t_phase = now;
            } else {
                if (g_ch[c].partner != IO_NO_CHAN) {
                    pin_safe(g_ch[c].partner);
                }
                g_ch[c].st = ST_IDLE;
            }
        }
        return 1;
    }
    if (g_ch[c].st == ST_LEAD || g_ch[c].st == ST_ON) {
        return 1;
    }
    if (!g_alive) {
        return refuse(IO_REFUSE_NO_HEARTBEAT);
    }
    if (g_ch[c].st == ST_LAG) {
        return refuse(IO_REFUSE_BUSY);
    }
    if (g_ch[c].off_valid && (u16)(now - g_ch[c].t_off) < g_ch[c].min_off) {
        return refuse(IO_REFUSE_MIN_OFF);
    }
    g_ch[c].t_mark = now;
    g_ch[c].t_phase = now;
    if (g_ch[c].partner != IO_NO_CHAN) {
        pin_active(g_ch[c].partner, 1);
        if (g_ch[c].lead) {
            g_ch[c].st = ST_LEAD;
            return 1;
        }
    }
    pin_active(c, 1);
    g_ch[c].st = ST_ON;
    return 1;
}

u8 io_heartbeat(u16 seq, u8 commanded)
{
    u8 reason = IO_REL_NONE;
    if (g_map_state != IO_MAP_VALID) {
        return 0;
    }
    if (g_have_seq && seq == g_last_seq) {
        reason = IO_REL_HB_SEQUENCE;
    } else if (commanded != keyed_mask()) {
        reason = IO_REL_HB_MISMATCH;
    }
    g_last_seq = seq;
    g_have_seq = 1;
    if (reason == IO_REL_NONE) {
        g_alive = 1;
        g_oos = 0;
        g_hb_t = now16();
    } else {
        g_alive = 0;
        g_oos = (commanded & (u8)~keyed_mask()) ? 1 : 0;
        io_release_all(reason);
    }
    return 1;
}

void io_tick(void)
{
    u8 c;
    u16 now = now16();
    if (g_map_state != IO_MAP_VALID) {
        return;
    }
    for (c = 0; c < IO_NCH; c++) {
        if (!is_primary(c)) {
            continue;
        }
        if (g_ch[c].st == ST_IDLE && g_ch[c].off_valid
            && (u16)(now - g_ch[c].t_off) >= g_ch[c].min_off) {
            g_ch[c].off_valid = 0;        /* the minimum off time is served; forget the stamp before it wraps */
        }
        switch (g_ch[c].st) {
        case ST_LEAD:
            if ((u16)(now - g_ch[c].t_phase) >= g_ch[c].lead) {
                pin_active(c, 1);
                g_ch[c].st = ST_ON;
            }
            /* fall through: the maximum on-time runs from the key-down command */
        case ST_ON:
            if (g_ch[c].func == IO_FUNC_PTT && (u16)(now - g_ch[c].t_mark) >= g_ch[c].max_on) {
                release_channel(c, IO_REL_TIMEOUT);
            }
            break;
        case ST_LAG:
            if ((u16)(now - g_ch[c].t_phase) >= g_ch[c].lag) {
                pin_safe(g_ch[c].partner);
                g_ch[c].st = ST_IDLE;
            }
            break;
        default:
            break;
        }
    }
    if (g_alive && (u16)(now - g_hb_t) >= g_hb_timeout) {
        g_alive = 0;
        io_release_all(IO_REL_HB_LAPSE);
    }
}

static void put16(u8 *d, u16 v)
{
    d[0] = (u8)v;
    d[1] = (u8)(v >> 8);
}

static u16 remaining(u8 c)
{
    u16 used;
    if (!is_primary(c) || g_ch[c].func != IO_FUNC_PTT
        || (g_ch[c].st != ST_LEAD && g_ch[c].st != ST_ON)) {
        return 0xFFFF;
    }
    used = (u16)(now16() - g_ch[c].t_mark);
    return used >= g_ch[c].max_on ? 0 : (u16)(g_ch[c].max_on - used);
}

u8 io_fill_status(u8 *d)
{
    u16 used;
    d[0] = IO_STATUS_LEN;
    d[1] = (u8)((g_map_state == IO_MAP_VALID ? 1 : 0) | (g_alive ? 2 : 0) | (g_oos ? 4 : 0)
                | (g_map_state == IO_MAP_LOADING ? 8 : 0));
    d[2] = keyed_mask();
    d[3] = active_mask();
    d[4] = g_rel_reason;
    d[5] = g_rel_chan;
    d[6] = g_rel_count;
    d[7] = g_map_error;
    d[8] = g_refusal;
    d[9] = g_ch[0].st;
    d[10] = g_ch[1].st;
    put16(d + 11, g_last_seq);
    put16(d + 13, now16());
    put16(d + 15, remaining(0));
    put16(d + 17, remaining(1));
    used = (u16)(now16() - g_hb_t);
    put16(d + 19, g_alive ? (used >= g_hb_timeout ? 0 : (u16)(g_hb_timeout - used)) : 0xFFFF);
    put16(d + 21, timer_loop_max());
    return IO_STATUS_LEN;
}

u8 io_fill_events(u8 *d)
{
    u8 n = g_rel_count < LOG_N ? g_rel_count : LOG_N;
    u8 i, k = 1;
    d[0] = g_rel_count;
    for (i = 0; i < n; i++) {               /* newest first */
        u8 s = (u8)(g_rel_count - 1 - i) & (LOG_N - 1);
        d[k++] = g_ev_reason[s];
        d[k++] = g_ev_chan[s];
        put16(d + k, g_ev_ms[s]);
        k += 2;
    }
    return k;
}

u8 io_fill_edges(u8 *d)
{
    u8 n = g_edge_count < LOG_N ? g_edge_count : LOG_N;
    u8 i, k = 1;
    d[0] = g_edge_count;
    for (i = 0; i < n; i++) {               /* newest first */
        u8 s = (u8)(g_edge_count - 1 - i) & (LOG_N - 1);
        d[k++] = g_edge_pl[s];
        put16(d + k, g_edge_ms[s]);
        put16(d + k + 2, g_edge_sub[s]);
        k += 4;
    }
    return k;
}
