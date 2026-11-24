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

#endif
