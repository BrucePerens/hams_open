# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
# xdata (the engine state) is placed at 0x3000, above the descriptors (0x2000-0x23xx) and below 0x4000.
# Build the Station I/O test image with SDCC 4.5.0 (mcs51). Output: build/station_io.ihx
CC = sdcc
CFLAGS = -mmcs51 --model-small --std-sdcc11 --opt-code-size -I core
BUILD = build
OBJS = $(BUILD)/station_io.rel $(BUILD)/fx2_core.rel $(BUILD)/descriptors.rel $(BUILD)/fx2_timer.rel $(BUILD)/io_engine.rel

all: $(BUILD)/station_io.ihx

$(BUILD)/%.rel: %.c core/*.h
	mkdir -p $(BUILD)
	$(CC) $(CFLAGS) -c -o $@ $<

$(BUILD)/%.rel: core/%.c core/*.h
	mkdir -p $(BUILD)
	$(CC) $(CFLAGS) -c -o $@ $<

$(BUILD)/station_io.ihx: $(OBJS)
	$(CC) -mmcs51 --model-small --xram-loc 0x3000 --xram-size 0x1000 -o $@ $(OBJS)

clean:
	rm -rf $(BUILD)

.PHONY: all clean
