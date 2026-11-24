/*
 * Whether controls held still count as use. A held button or a stick kept
 * pushed sends no further events, so gizlet-power also looks at what is held
 * now before it dims or turns off the screen. Plain C, without Linux headers,
 * so idle_test.c runs on any computer.
 */
#ifndef GIZLET_POWER_IDLE_H
#define GIZLET_POWER_IDLE_H

#include <stddef.h>
#include <stdlib.h>

/* Whether any bit of a key-state bitmap (EVIOCGKEY) is set. */
static inline int idle_any_key(const unsigned char *keys, size_t bytes) {
    for (size_t i = 0; i < bytes; i++) {
        if (keys[i]) return 1;
    }
    return 0;
}

/* Whether an axis is in use now. A hat is in use off center; a stick or a
 * trigger when it is more than an eighth of its range from where it rests,
 * its value when the device was opened, so sensor noise and drift do not
 * count. */
static inline int idle_axis_engaged(int value, int rest, int min, int max, int hat) {
    if (hat) return value != 0;
    int range = max - min;
    return range > 0 && abs(value - rest) * 8 >= range;
}

enum screen_state { SCREEN_ON, SCREEN_DIM, SCREEN_OFF };

/* The screen's state at NOW (ms), from the one it is in. SECONDS is the
 * screen-off timeout, 0 for never; it dims DIM_SECONDS before, when the
 * timeout is long enough. HELD says a control is held now: that is use, so
 * the screen never turns off, and the inputs are never grabbed, while a
 * button is down that an app has seen pressed and must see released.
 * *LAST_USE is moved to NOW then. Waking from off is left to presses. */
static inline enum screen_state idle_next(enum screen_state s, long long now, long long *last_use, int seconds,
                                          int dim_seconds, int held) {
    if (seconds == 0) return SCREEN_ON;
    if (s == SCREEN_OFF) return SCREEN_OFF;
    if (held) {
        *last_use = now;
        return SCREEN_ON;
    }
    long long off_at = *last_use + (long long)seconds * 1000;
    long long dim_at = off_at - (seconds > dim_seconds * 2 ? (long long)dim_seconds * 1000 : 0);
    if (now >= off_at) return SCREEN_OFF;
    if (now >= dim_at) return SCREEN_DIM;
    return s;
}

#endif
