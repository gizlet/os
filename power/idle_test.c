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

    /* The timeout and wake sequence, 60 s timeout, dimming 15 s before. */
    long long last = 0;
    enum screen_state st = SCREEN_ON;
    st = idle_next(st, 44000, &last, 60, 15, 0);
    check(st == SCREEN_ON, "on before the dim");
    st = idle_next(st, 45000, &last, 60, 15, 0);
    check(st == SCREEN_DIM, "dims 15 s before");
    /* A button held through the timeout, such as accelerate: never off, and
     * so never grabbed while the app still sees it down. */
    st = idle_next(st, 60000, &last, 60, 15, 1);
    check(st == SCREEN_ON && last == 60000, "held through the timeout stays on");
    st = idle_next(st, 119000, &last, 60, 15, 1);
    check(st == SCREEN_ON && last == 119000, "held for a minute more stays on");
    /* Released: the timeout counts from the last time it was held. */
    st = idle_next(st, 178999, &last, 60, 15, 0);
    check(st == SCREEN_DIM, "dimmed after the release");
    st = idle_next(st, 179000, &last, 60, 15, 0);
    check(st == SCREEN_OFF, "off a full timeout after the release");
    /* Off stays off until a press, which the daemon handles itself. */
    check(idle_next(st, 999999, &last, 60, 15, 1) == SCREEN_OFF, "off waits for a press");
    check(idle_next(st, 0, &last, 0, 15, 0) == SCREEN_ON, "never turns back on");

    /* Rest positions come from the range: a stick held when the device
     * was opened still rests in the middle once it is let go. */
    int rest = idle_axis_rest(0, 1023, 0, 1023);
    check(rest == 511, "a stick held at open rests in the middle");
    check(!idle_axis_engaged(512, rest, 0, 1023, 0), "the released stick is not held");
    check(idle_axis_engaged(1023, rest, 0, 1023, 0), "the stick pushed again is held");
    check(idle_axis_rest(1, 3, 0, 255) == 0, "a trigger rests at its minimum");
    check(idle_axis_rest(1, 128, 0, 255) == 127, "a second stick on ABS_Z rests in the middle");
    /* Held without any new input counts for ten minutes at most. */
    check(idle_held_counts(IDLE_HELD_LIMIT_MS - 1, 0), "held within the limit");
    check(!idle_held_counts(IDLE_HELD_LIMIT_MS, 0), "held past the limit");

    /* A stick held when the inputs were grabbed for sleep and let go during
     * it: readers are told its old value, which the kernel would otherwise
     * drop as a repeat, then where it is now. */
    int values[2];
    check(idle_resync(1023, 512, values) == 2 && values[0] == 1023 && values[1] == 512, "a stick let go while grabbed is replayed");
    check(idle_resync(512, 512, values) == 0, "an unmoved stick is not");

    if (failures) return 1;
    puts("idle_test: sustained input counts as use");
    return 0;
}
