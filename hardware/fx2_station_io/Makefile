# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
# Build the Station I/O test image with SDCC 4.5.0 (mcs51). Output: build/station_io.ihx
CC = sdcc
CFLAGS = -mmcs51 --model-small --std-sdcc11 --opt-code-size -I core
BUILD = build
OBJS = $(BUILD)/station_io.rel $(BUILD)/fx2_core.rel $(BUILD)/descriptors.rel $(BUILD)/fx2_timer.rel

all: $(BUILD)/station_io.ihx

$(BUILD)/%.rel: %.c core/*.h
	mkdir -p $(BUILD)
	$(CC) $(CFLAGS) -c -o $@ $<

$(BUILD)/%.rel: core/%.c core/*.h
	mkdir -p $(BUILD)
	$(CC) $(CFLAGS) -c -o $@ $<

$(BUILD)/station_io.ihx: $(OBJS)
	$(CC) -mmcs51 --model-small -o $@ $(OBJS)

clean:
	rm -rf $(BUILD)

.PHONY: all clean
