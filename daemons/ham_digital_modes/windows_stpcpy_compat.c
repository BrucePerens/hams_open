// SPDX-License-Identifier: LGPL-3.0-or-later
//
// `stpcpy` is a POSIX/glibc extension (copies a string and returns a pointer to its terminating
// NUL, rather than plain `strcpy`'s pointer to the start) -- mingw-w64's own headers genuinely
// never declare or define it at all (confirmed directly: grepped
// /usr/x86_64-w64-mingw32/include/string.h, no match), not merely gated behind a feature-test
// macro the way some POSIX extensions are. vendor/ft8_lib/ft8/message.c (third-party, MIT-
// licensed, not ours to edit) calls it once, in unpackgrid(). Only compiled in for Windows
// targets (see build.rs) -- every other platform this daemon builds for already provides a real
// stpcpy via its own libc.
#if defined(_WIN32) || defined(__MINGW32__) || defined(__MINGW64__)
#include <string.h>
#include "windows_stpcpy_compat.h"

char *stpcpy(char *dst, const char *src) {
    size_t len = strlen(src);
    memcpy(dst, src, len + 1);
    return dst + len;
}
#endif
