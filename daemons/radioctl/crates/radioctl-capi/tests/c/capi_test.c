/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* A C program that uses radioctl only through include/radioctl.h: runtime ownership, a virtual rig,
 * frequency and mode, and the transmit safety rules (gate closed by default, no key-down without
 * the explicit permit call, unkey always allowed). Built with -std=c99 -Wall -Wextra -Werror
 * -pedantic by tests/c_program.rs. Usage: capi_test RIGFILE.toml */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "radioctl.h"

static int failures = 0;

#define CHECK(cond)                                                              \
    do {                                                                         \
        if (!(cond)) {                                                           \
            fprintf(stderr, "FAILED %s:%d: %s (last error: %s)\n", __FILE__,     \
                    __LINE__, #cond, rctl_last_error());                         \
            failures++;                                                          \
        }                                                                        \
    } while (0)

static char *slurp(const char *path) {
    FILE *f = fopen(path, "rb");
    long n;
    char *buf;
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    n = ftell(f);
    fseek(f, 0, SEEK_SET);
    buf = (char *)malloc((size_t)n + 1);
    if (buf && fread(buf, 1, (size_t)n, f) == (size_t)n) {
        buf[n] = 0;
    } else {
        free(buf);
        buf = NULL;
    }
    fclose(f);
    return buf;
}

int main(int argc, char **argv) {
    rctl_runtime *rt = NULL;
    rctl_rig *rig = NULL;
    uint64_t hz = 0;
    int on = -1, open = -1, up = -1;
    char mode[16];
    char tiny[2];
    char *toml;

    if (argc != 2) {
        fprintf(stderr, "usage: capi_test RIGFILE.toml\n");
        return 2;
    }
    toml = slurp(argv[1]);
    if (!toml) {
        fprintf(stderr, "cannot read %s\n", argv[1]);
        return 2;
    }

    /* Version and error plumbing. */
    CHECK(rctl_abi_version() == (uint32_t)RCTL_ABI_VERSION);
    CHECK(strlen(rctl_version()) > 0);
    CHECK(rctl_last_error() != NULL);

    /* Nulls and stale handles are errors, never crashes. */
    CHECK(rctl_runtime_new(NULL) == RCTL_ERR_NULL);
    CHECK(strstr(rctl_last_error(), "null") != NULL);
    CHECK(rctl_rig_get_frequency(NULL, 0, &hz) == RCTL_ERR_NULL);
    CHECK(rctl_rig_close(NULL) == RCTL_ERR_NULL);
    CHECK(rctl_runtime_free(NULL) == RCTL_ERR_NULL);

    /* The caller owns the runtime explicitly. */
    CHECK(rctl_runtime_new(&rt) == RCTL_OK);
    CHECK(rt != NULL);
    CHECK(rctl_rig_open_virtual(rt, "not a rig file", &rig) == RCTL_ERR_INVALID_ARG);
    CHECK(rig == NULL);
    CHECK(rctl_rig_open_virtual(rt, NULL, &rig) == RCTL_ERR_NULL);
    CHECK(rctl_rig_open_virtual(rt, toml, &rig) == RCTL_OK);
    CHECK(rig != NULL);
    if (!rig) return 1;

    /* A runtime with an open rig cannot be freed. */
    CHECK(rctl_runtime_free(rt) == RCTL_ERR_BUSY);

    /* Reading and tuning work with the gate closed: they do not transmit. */
    CHECK(rctl_rig_link_up(rig, &up) == RCTL_OK && up == 1);
    CHECK(rctl_rig_set_frequency(rig, 0, 7030000u) == RCTL_OK);
    CHECK(rctl_rig_get_frequency(rig, 0, &hz) == RCTL_OK);
    CHECK(hz == 7030000u);
    CHECK(rctl_rig_get_frequency(rig, 7, &hz) == RCTL_ERR_INVALID_ARG);
    CHECK(rctl_rig_set_frequency(rig, 0, 0) == RCTL_ERR_INVALID_ARG);
    CHECK(rctl_rig_set_mode(rig, "usb") == RCTL_OK);
    CHECK(rctl_rig_get_mode(rig, mode, sizeof mode) == RCTL_OK);
    CHECK(strcmp(mode, "usb") == 0);
    CHECK(rctl_rig_get_mode(rig, tiny, sizeof tiny) == RCTL_ERR_BUFFER_TOO_SMALL);
    CHECK(rctl_rig_set_mode(rig, "no-such-mode") == RCTL_ERR_INVALID_ARG);

    /* The transmit gate is closed by default. */
    CHECK(rctl_rig_transmit_permitted(rig, &open) == RCTL_OK && open == 0);
    CHECK(rctl_rig_get_ptt(rig, &on) == RCTL_OK && on == 0);
    CHECK(rctl_rig_set_ptt(rig, 1) == RCTL_ERR_TX_GATE_CLOSED);
    CHECK(strstr(rctl_last_error(), "rctl_rig_permit_transmit") != NULL);
    CHECK(rctl_rig_get_ptt(rig, &on) == RCTL_OK && on == 0);
    /* Unkeying is always allowed. */
    CHECK(rctl_rig_set_ptt(rig, 0) == RCTL_OK);

    /* The permit call names who decided; an empty answer does not open the gate. */
    CHECK(rctl_rig_permit_transmit(rig, "") == RCTL_ERR_INVALID_ARG);
    CHECK(rctl_rig_permit_transmit(rig, "   ") == RCTL_ERR_INVALID_ARG);
    CHECK(rctl_rig_permit_transmit(rig, NULL) == RCTL_ERR_NULL);
    CHECK(rctl_rig_transmit_permitted(rig, &open) == RCTL_OK && open == 0);
    CHECK(rctl_rig_set_ptt(rig, 1) == RCTL_ERR_TX_GATE_CLOSED);

    /* With the explicit permit the virtual rig keys; revoking closes the gate again. */
    CHECK(rctl_rig_permit_transmit(rig, "capi_test.c, virtual rig, nothing on the air") == RCTL_OK);
    CHECK(rctl_rig_transmit_permitted(rig, &open) == RCTL_OK && open == 1);
    CHECK(rctl_rig_set_ptt(rig, 1) == RCTL_OK);
    CHECK(rctl_rig_get_ptt(rig, &on) == RCTL_OK && on == 1);
    CHECK(rctl_rig_revoke_transmit(rig) == RCTL_OK);
    CHECK(rctl_rig_set_ptt(rig, 1) == RCTL_ERR_TX_GATE_CLOSED);
    CHECK(rctl_rig_set_ptt(rig, 0) == RCTL_OK);
    CHECK(rctl_rig_get_ptt(rig, &on) == RCTL_OK && on == 0);

    /* Close, then the runtime can go. A second close of the same handle is the caller's bug. */
    CHECK(rctl_rig_close(rig) == RCTL_OK);
    CHECK(rctl_runtime_free(rt) == RCTL_OK);

    /* A runtime for a TCP rig that nobody listens on: a clean, bounded error. */
    rt = NULL;
    rig = NULL;
    CHECK(rctl_runtime_new(&rt) == RCTL_OK);
    CHECK(rctl_rig_open_tcp(rt, toml, "127.0.0.1:1", &rig) == RCTL_ERR_LINK_DOWN);
    CHECK(rig == NULL);
    CHECK(rctl_rig_open_serial(rt, toml, "/dev/null/not-a-port", 0, &rig) == RCTL_ERR_INVALID_ARG);
    CHECK(rctl_rig_open_serial(rt, toml, "/dev/null/not-a-port", 9600, &rig) == RCTL_ERR_LINK_DOWN);
    CHECK(rctl_runtime_free(rt) == RCTL_OK);

    free(toml);
    if (failures) {
        fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    printf("OK\n");
    return 0;
}
