/* Sustained input keeps the screen on: held buttons and pushed sticks count. */
#include "idle.h"

#include <stdio.h>

static int failures;

static void check(int ok, const char *what) {
    if (!ok) {
        fprintf(stderr, "FAIL: %s\n", what);
        failures++;
    }
}

int main(void) {
    unsigned char keys[96] = {0};
    check(!idle_any_key(keys, sizeof keys), "nothing held");
    keys[0x131 / 8] |= 1 << (0x131 % 8); /* BTN_EAST held, as while accelerating */
    check(idle_any_key(keys, sizeof keys), "a held button");

    /* A centered stick of 0..1023 resting at 512. */
    check(!idle_axis_engaged(512, 512, 0, 1023, 0), "stick at rest");
    check(!idle_axis_engaged(560, 512, 0, 1023, 0), "stick noise");
    check(idle_axis_engaged(1023, 512, 0, 1023, 0), "stick kept pushed right");
    check(idle_axis_engaged(0, 512, 0, 1023, 0), "stick kept pushed left");
    /* A trigger of 0..255 resting at 0. */
    check(!idle_axis_engaged(10, 0, 0, 255, 0), "trigger at rest");
    check(idle_axis_engaged(255, 0, 0, 255, 0), "trigger held down");
    /* A D-pad hat. */
    check(!idle_axis_engaged(0, 0, -1, 1, 1), "hat centered");
    check(idle_axis_engaged(-1, 0, -1, 1, 1), "hat held");
    /* An axis without a range never counts. */
    check(!idle_axis_engaged(5, 0, 0, 0, 0), "no range");

    if (failures) return 1;
    puts("idle_test: sustained input counts as use");
    return 0;
}
