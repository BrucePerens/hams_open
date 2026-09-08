// SPDX-License-Identifier: LGPL-3.0-or-later
//
// A real prototype for stpcpy(), forced into scope via `-include` for every file this crate's
// build.rs compiles when targeting Windows -- see windows_stpcpy_compat.c for why the function
// itself doesn't exist in mingw-w64's own headers at all. `.warnings(false)` (`-w`) does NOT
// suppress GCC 14's own "implicit declaration of function" diagnostic for a call with no
// prototype in scope (confirmed directly: it fires as a hard error regardless of -w or -std=),
// so the real fix is making a real prototype visible, not fighting that diagnostic's severity.
#ifndef HAMS_WINDOWS_STPCPY_COMPAT_H
#define HAMS_WINDOWS_STPCPY_COMPAT_H

#include <stddef.h>

char *stpcpy(char *dst, const char *src);

#endif
