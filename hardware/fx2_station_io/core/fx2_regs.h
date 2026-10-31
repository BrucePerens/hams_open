/* SPDX-License-Identifier: AGPL-3.0-or-later */
/*
 * Copyright (C) 2026 Bruce Perens and the hams.com contributors.
 * Part of the hams_open FX2LP shared firmware core.
 *
 * Register map for the Cypress/Infineon CY7C68013A (EZ-USB FX2LP), 56-pin part,
 * written from "EZ-USB Technical Reference Manual", Document # 001-13670 Rev. *D
 * (cited below as TRM). Register addresses are facts about the chip; the names
 * follow the TRM. No fx2lafw or fx2lib source was used.
 */
#ifndef FX2_REGS_H
#define FX2_REGS_H

typedef unsigned char u8;
typedef unsigned int u16;     /* SDCC mcs51: int is 16 bits */
typedef unsigned long u32;

#define XREG(addr) (*(volatile __xdata u8 *)(addr))

/* TRM 15.5.1 CPUCS (0xE600). Only the host writes 8051RES (via request 0xA0). */
#define CPUCS XREG(0xE600)
#define CPUCS_CLKSPD_48MHZ 0x10  /* CLKSPD[1:0] = 10 (TRM 15.5.1) */
#define CPUCS_CLKOE 0x02         /* drive CLKOUT pin; reset value 1 */

/* TRM 15.5.2 IFCONFIG (0xE601): reset value 0x80 = internal 30 MHz IFCLK, IFCFG=00 ports. */
#define IFCONFIG XREG(0xE601)
#define IFCONFIG_PORTS 0x80

/* TRM 15.6.1 onward: endpoint configuration. VALID is bit 7 (EP1OUTCFG reset value 0xA0). */
#define EP1OUTCFG XREG(0xE610)
#define EP1INCFG  XREG(0xE611)
#define EP2CFG    XREG(0xE612)
#define EP4CFG    XREG(0xE613)
#define EP6CFG    XREG(0xE614)
#define EP8CFG    XREG(0xE615)

/* TRM 13.4 / 15.8.1: alternate-function selection for port A (0 = plain I/O). */
#define PORTACFG XREG(0xE670)

/* TRM 15.11.1 USBCS (0xE680). DISCON and RENUM drive ReNumeration (TRM 3.9). */
#define USBCS XREG(0xE680)
#define USBCS_DISCON 0x08
#define USBCS_RENUM  0x02

/* TRM 15.7.8: USB interrupt request flags, polled here (write 1 to clear). */
#define USBIRQ XREG(0xE65D)
#define USBIRQ_URES  0x10   /* bus reset */
#define USBIRQ_SUDAV 0x01   /* setup data available */
#define USBIRQ_SUSP  0x08   /* 3 ms of no bus activity: suspend signalling (TRM 15.7.8) */

/* TRM 2.2, 15.12.10 EP0CS, 15.12.26 SUDPTR: endpoint zero. */
#define EP0BCH XREG(0xE68A)
#define EP0BCL XREG(0xE68B)
#define EP0CS  XREG(0xE6A0)
#define EP0CS_HSNAK 0x80    /* write 1 to clear: ACK the status stage */
#define EP0CS_STALL 0x01
#define SUDPTRH   XREG(0xE6B3)
#define SUDPTRL   XREG(0xE6B4)
#define SUDPTRCTL XREG(0xE6B5)  /* bit 0 SDPAUTO: 0 = firmware loads EP0BCH:L (TRM 15.12.27) */
#define SETUPDAT  ((volatile __xdata u8 *)0xE6B8)
#define EP0BUF    ((volatile __xdata u8 *)0xE740)

/* TRM 13.2 / SFR table 15-1: I/O ports. IOx reads pin state; OEx 1 = output. */
__sfr __at(0x80) IOA;
__sfr __at(0x90) IOB;
__sfr __at(0xB0) IOD;
__sfr __at(0xB2) OEA;
__sfr __at(0xB3) OEB;
__sfr __at(0xB5) OED;

/* TRM chapter 14 (Timers/Counters): TCON 0x88, TMOD 0x89, TL0/TL1/TH0/TH1 0x8A-0x8D, CKCON 0x8E
 * (T2M bit 5, T1M bit 4, T0M bit 3: 0 = CLKOUT/12, 1 = CLKOUT/4; Table 14-4), T2CON 0xC8,
 * RCAP2L/H 0xCA/0xCB, TL2/TH2 0xCC/0xCD. Timer 2 in 16-bit auto-reload mode (CP/RL2 = 0,
 * TR2 = 1, TF2 set on overflow, cleared by software; Tables 14-5 and 14-6). */
__sfr __at(0x88) TCON;
__sfr __at(0x89) TMOD;
__sfr __at(0x8A) TL0;
__sfr __at(0x8B) TL1;
__sfr __at(0x8C) TH0;
__sfr __at(0x8D) TH1;
__sfr __at(0x8E) CKCON;
__sfr __at(0xC8) T2CON;
__sfr __at(0xCA) RCAP2L;
__sfr __at(0xCB) RCAP2H;
__sfr __at(0xCC) TL2;
__sfr __at(0xCD) TH2;
#define TCON_TF0 0x20
#define TCON_TR0 0x10
#define TCON_TF1 0x80
#define TCON_TR1 0x40
#define T2CON_TF2 0x80
#define T2CON_TR2 0x04
#define TMOD_T0_MODE1 0x01      /* 16-bit counter, TMOD.1:0 = 01 (Table 14-2) */
#define TMOD_T1_MODE1 0x10
#define CKCON_T2M 0x20
#define CKCON_T1M 0x10
#define CKCON_T0M 0x08

/* TRM 15.15: registers in 0xE600-0xE6FF need a few CPU cycles between some accesses. */
#define SYNCDELAY() do { __asm nop __endasm; __asm nop __endasm; __asm nop __endasm; __asm nop __endasm; } while (0)

#endif
