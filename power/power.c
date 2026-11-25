/*
 * gizlet-power [CONFIG]: puts the device to sleep (suspend to RAM) when it is
 * idle, on a press of the power key, or on SIGUSR1 (the launcher's Sleep).
 *
 * Every input device with buttons or axes is read alongside its other
 * readers. After `sleep_after` seconds without input (CONFIG, by default
 * /run/gizlet-data/system/power.ini, which the launcher writes; 0 is never,
 * 300 when unset) the device sleeps; 15 s before that the screen dims, and
 * any input brings the brightness back.
 *
 * - Sleep waits until every control is released (buttons up, sticks back at
 *   rest), so no app is left seeing one held. The backlight turns off and
 *   the input devices are grabbed, so the press that wakes the device reaches
 *   no app; the grab is released a second after waking, once every button is
 *   up.
 * - When the kernel cannot sleep, the screen stays off instead, and a button
 *   press turns it on again. Sticks do not wake it.
 *
 * It runs on the system, not in the launcher, so the timer also runs while an
 * app has the screen. The file is read again whenever it changes.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/input.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#include "idle.h"

#define MAX_DEVICES 32
#define DIM_SECONDS 15
#define DEFAULT_SLEEP_AFTER 300
#define RESCAN_MS 2000
/* After waking, input is ignored this long: the power key's own press and
 * release arrive as the device resumes. */
#define SETTLE_MS 1000
/* How often a pending sleep checks whether the controls are released, and how
 * long it waits for sticks: one whose rest was guessed wrong must not keep the
 * device awake, and a stick still held is replayed when the grab ends
 * (resync). Buttons report their state exactly, so it always waits for
 * them. */
#define PENDING_MS 50
#define PENDING_LIMIT_MS 10000
#define BITS(n) (((n) + 7) / 8)

struct device {
    int fd;
    char name[32];
    unsigned char abs[BITS(ABS_CNT)];
    struct input_absinfo info[ABS_CNT];
    int last[ABS_CNT];
    /* Where each axis rests (idle_axis_rest). */
    int rest[ABS_CNT];
    /* Each axis's value when the inputs were grabbed: what other readers saw. */
    int seen[ABS_CNT];
};

static struct device devices[MAX_DEVICES];
static int ndevices;
static char backlight[256];
static enum screen_state screen = SCREEN_ON;
static int saved_brightness = -1;
static int grabbed;
static volatile sig_atomic_t stopping, sleep_requested;

static long long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

static int test_bit(const unsigned char *bits, int n) {
    return (bits[n / 8] >> (n % 8)) & 1;
}

static int read_int(const char *dir, const char *file) {
    char path[320], buf[32];
    snprintf(path, sizeof path, "%s/%s", dir, file);
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return -1;
    buf[n] = 0;
    return atoi(buf);
}

static void write_int(const char *dir, const char *file, int value) {
    char path[320], buf[32];
    snprintf(path, sizeof path, "%s/%s", dir, file);
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0) return;
    int n = snprintf(buf, sizeof buf, "%d\n", value);
    if (write(fd, buf, n) != n) fprintf(stderr, "gizlet-power: cannot write %s: %s\n", path, strerror(errno));
    close(fd);
}

/* The first backlight in name order. */
static int find_backlight(void) {
    struct dirent **names;
    int n = scandir("/sys/class/backlight", &names, NULL, alphasort);
    if (n < 0) return 0;
    int found = 0;
    for (int i = 0; i < n; i++) {
        if (!found && names[i]->d_name[0] != '.') {
            snprintf(backlight, sizeof backlight, "/sys/class/backlight/%.200s", names[i]->d_name);
            found = 1;
        }
        free(names[i]);
    }
    free(names);
    return found;
}

/* sleep_after from CONFIG, or screen_off, its earlier name, without it;
 * reread only when the file changes. */
static int sleep_after_seconds(const char *config) {
    static struct timespec seen = {-1, 0};
    static int seconds = DEFAULT_SLEEP_AFTER;
    struct stat st;
    if (stat(config, &st) != 0) {
        seconds = DEFAULT_SLEEP_AFTER;
        seen.tv_sec = -1;
        return seconds;
    }
    if (st.st_mtim.tv_sec == seen.tv_sec && st.st_mtim.tv_nsec == seen.tv_nsec) return seconds;
    seen = st.st_mtim;
    seconds = DEFAULT_SLEEP_AFTER;
    FILE *f = fopen(config, "re");
    if (!f) return seconds;
    char line[128];
    int found = 0, old = -1;
    while (fgets(line, sizeof line, f)) {
        int v;
        if (sscanf(line, " sleep_after = %d", &v) == 1 && v >= 0) {
            seconds = v;
            found = 1;
        }
        /* The key before sleep_after, read when that is absent. */
        if (sscanf(line, " screen_off = %d", &v) == 1 && v >= 0) old = v;
    }
    fclose(f);
    if (!found && old >= 0) seconds = old;
    printf("gizlet-power: sleep after %s%d s\n", seconds ? "" : "never, ", seconds);
    fflush(stdout);
    return seconds;
}

/* Tells every reader of D where its axes are now, if they moved while it was
 * grabbed (idle_resync). Readers that cache axis values from events, such as
 * the launcher, would otherwise keep a stick held that was let go. */
static void resync(struct device *d) {
    struct input_event ev[2 * ABS_CNT + 2];
    int n = 0, second[ABS_CNT], moved = 0;
    for (int a = 0; a < ABS_CNT; a++) {
        struct input_absinfo now;
        int values[2];
        second[a] = INT_MIN;
        if (!test_bit(d->abs, a) || ioctl(d->fd, EVIOCGABS(a), &now) < 0 || !idle_resync(d->seen[a], now.value, values)) continue;
        ev[n++] = (struct input_event){.type = EV_ABS, .code = a, .value = values[0]};
        second[a] = values[1];
        moved = 1;
    }
    if (!moved) return;
    ev[n++] = (struct input_event){.type = EV_SYN, .code = SYN_REPORT};
    for (int a = 0; a < ABS_CNT; a++) {
        if (second[a] != INT_MIN) ev[n++] = (struct input_event){.type = EV_ABS, .code = a, .value = second[a]};
    }
    ev[n++] = (struct input_event){.type = EV_SYN, .code = SYN_REPORT};
    if (write(d->fd, ev, n * sizeof ev[0]) < 0) fprintf(stderr, "gizlet-power: cannot resync %s: %s\n", d->name, strerror(errno));
}

static void grab(int on) {
    for (int i = 0; i < ndevices; i++) {
        struct device *d = &devices[i];
        if (on) {
            for (int a = 0; a < ABS_CNT; a++) {
                struct input_absinfo now;
                if (test_bit(d->abs, a) && ioctl(d->fd, EVIOCGABS(a), &now) >= 0) d->seen[a] = now.value;
            }
        }
        ioctl(d->fd, EVIOCGRAB, on);
        if (!on) resync(d);
    }
    grabbed = on;
}

static void set_screen(enum screen_state next) {
    if (next == screen) return;
    if (screen == SCREEN_ON) saved_brightness = read_int(backlight, "brightness");
    int max = read_int(backlight, "max_brightness");
    if (next == SCREEN_ON) {
        write_int(backlight, "bl_power", 0);
        if (saved_brightness > 0) write_int(backlight, "brightness", saved_brightness);
    } else if (next == SCREEN_DIM) {
        int dim = max > 0 ? max / 10 : 1;
        if (dim < 1) dim = 1;
        if (saved_brightness > 0 && dim > saved_brightness) dim = saved_brightness;
        write_int(backlight, "bl_power", 0);
        write_int(backlight, "brightness", dim);
    } else {
        write_int(backlight, "bl_power", 4);
        write_int(backlight, "brightness", 0);
        grab(1);
    }
    static const char *names[] = {"on", "dimmed", "off"};
    printf("gizlet-power: screen %s\n", names[next]);
    fflush(stdout);
    screen = next;
}

/* Whether a button was pressed since the inputs were grabbed. The events are
 * read and dropped: no app sees them. */
static int pressed_while_grabbed(void) {
    int pressed = 0;
    for (int i = 0; i < ndevices; i++) {
        struct input_event ev[64];
        ssize_t len;
        while ((len = read(devices[i].fd, ev, sizeof ev)) > 0) {
            for (size_t k = 0; k < (size_t)len / sizeof ev[0]; k++) pressed |= ev[k].type == EV_KEY && ev[k].value == 1;
        }
    }
    return pressed;
}

enum suspend_result { SUSPEND_FAILED, SUSPEND_SLEPT, SUSPEND_CANCELLED };

/* Turns the screen off and suspends to RAM; returns once the device wakes.
 * A press or a kernel wakeup event while it gets ready (sync can take
 * seconds) cancels it, through /sys/power/wakeup_count. When it cannot
 * sleep, the screen stays off. */
static enum suspend_result suspend(void) {
    /* Reading blocks while a wakeup event is in progress. */
    char count[32];
    ssize_t count_len = -1;
    int cfd = open("/sys/power/wakeup_count", O_RDONLY | O_CLOEXEC);
    if (cfd >= 0) {
        count_len = read(cfd, count, sizeof count - 1);
        close(cfd);
    }
    set_screen(SCREEN_OFF);
    sync();
    int cancelled = pressed_while_grabbed();
    /* Writing the count back fails when a wakeup event came since. */
    if (!cancelled && count_len > 0) {
        cfd = open("/sys/power/wakeup_count", O_WRONLY | O_CLOEXEC);
        cancelled = cfd >= 0 && write(cfd, count, count_len) != count_len;
        if (cfd >= 0) close(cfd);
    }
    int slept = 0, err = 0;
    if (!cancelled) {
        int fd = open("/sys/power/state", O_WRONLY | O_CLOEXEC);
        slept = fd >= 0 && write(fd, "mem\n", 4) == 4;
        err = errno;
        if (fd >= 0) close(fd);
        /* The kernel gave up on a wakeup event during suspend. */
        if (!slept && err == EBUSY) cancelled = 1;
    }
    if (cancelled) {
        puts("gizlet-power: sleep cancelled by a wakeup");
    } else if (slept) {
        puts("gizlet-power: woke up");
    } else {
        fprintf(stderr, "gizlet-power: cannot sleep: %s; the screen stays off\n", strerror(err));
    }
    fflush(stdout);
    return cancelled ? SUSPEND_CANCELLED : slept ? SUSPEND_SLEPT : SUSPEND_FAILED;
}

static void close_device(int i) {
    close(devices[i].fd);
    devices[i] = devices[--ndevices];
}

/* Opens input devices with buttons or axes that are not open yet. */
static void rescan(void) {
    DIR *dir = opendir("/dev/input");
    if (!dir) return;
    struct dirent *e;
    while (ndevices < MAX_DEVICES && (e = readdir(dir))) {
        if (strncmp(e->d_name, "event", 5) != 0) continue;
        int open_already = 0;
        for (int i = 0; i < ndevices; i++) open_already |= strcmp(devices[i].name, e->d_name) == 0;
        if (open_already) continue;
        /* Writable to replay axes after a grab (resync). */
        int fd = openat(dirfd(dir), e->d_name, O_RDWR | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) fd = openat(dirfd(dir), e->d_name, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) continue;
        unsigned char types[BITS(EV_CNT)] = {0};
        /* Switches are left out: grabbing the headphone jack would hide it
         * from gizlet-audio-jack. */
        if (ioctl(fd, EVIOCGBIT(0, sizeof types), types) < 0 || !(test_bit(types, EV_KEY) || test_bit(types, EV_ABS)) ||
            test_bit(types, EV_SW)) {
            close(fd);
            continue;
        }
        struct device *d = &devices[ndevices++];
        memset(d, 0, sizeof *d);
        d->fd = fd;
        snprintf(d->name, sizeof d->name, "%.31s", e->d_name);
        if (test_bit(types, EV_ABS) && ioctl(fd, EVIOCGBIT(EV_ABS, sizeof d->abs), d->abs) >= 0) {
            for (int a = 0; a < ABS_CNT; a++) {
                if (test_bit(d->abs, a) && ioctl(fd, EVIOCGABS(a), &d->info[a]) >= 0) {
                    int trigger = a == ABS_Z || a == ABS_RZ || a == ABS_GAS || a == ABS_BRAKE;
                    d->last[a] = d->seen[a] = d->info[a].value;
                    d->rest[a] = idle_axis_rest(trigger, d->info[a].value, d->info[a].minimum, d->info[a].maximum);
                }
            }
        }
        if (grabbed) ioctl(fd, EVIOCGRAB, 1);
    }
    closedir(dir);
}

/* Whether a button is down on any device. */
static int any_key_down(void) {
    for (int i = 0; i < ndevices; i++) {
        unsigned char keys[BITS(KEY_CNT)] = {0};
        if (ioctl(devices[i].fd, EVIOCGKEY(sizeof keys), keys) >= 0 && idle_any_key(keys, sizeof keys)) return 1;
    }
    return 0;
}

/* Whether a control is held now: a button down, a hat off center, or a stick
 * or trigger kept away from rest. These send no events while held. */
static int controls_held(void) {
    if (any_key_down()) return 1;
    for (int i = 0; i < ndevices; i++) {
        struct device *d = &devices[i];
        for (int a = 0; a < ABS_CNT; a++) {
            struct input_absinfo now;
            if (!test_bit(d->abs, a) || ioctl(d->fd, EVIOCGABS(a), &now) < 0) continue;
            int hat = a >= ABS_HAT0X && a <= ABS_HAT3Y;
            if (idle_axis_engaged(now.value, d->rest[a], d->info[a].minimum, d->info[a].maximum, hat)) return 1;
        }
    }
    return 0;
}

/* Whether EV counts as use: a button press, a hat, or a stick or trigger moved
 * by an eighth of its range since the last use, so sensor noise does not.
 * *PRESSED is set on a press, *POWER on a press of the power key. */
static int is_activity(struct device *d, const struct input_event *ev, int *pressed, int *power) {
    if (ev->type == EV_KEY) {
        if (ev->value == 1) *pressed = 1;
        if (ev->value == 1 && ev->code == KEY_POWER) *power = 1;
        return ev->value != 0;
    }
    if (ev->type != EV_ABS || ev->code >= ABS_CNT) return 0;
    if (ev->code >= ABS_HAT0X && ev->code <= ABS_HAT3Y) return ev->value != 0;
    int range = d->info[ev->code].maximum - d->info[ev->code].minimum;
    int moved = abs(ev->value - d->last[ev->code]);
    if (range > 0 && moved * 8 < range) return 0;
    d->last[ev->code] = ev->value;
    return 1;
}

static void on_signal(int sig) {
    if (sig == SIGUSR1) {
        sleep_requested = 1;
    } else {
        stopping = 1;
    }
}

int main(int argc, char **argv) {
    const char *config = argc > 1 ? argv[1] : "/run/gizlet-data/system/power.ini";
    if (!find_backlight()) {
        fputs("gizlet-power: no backlight\n", stderr);
        return 0;
    }
    struct sigaction sa = {.sa_handler = on_signal};
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGUSR1, &sa, NULL);

    long long last_use = now_ms(), last_event = last_use, last_scan = 0, settle_until = 0;
    /* A sleep asked for by the power key or SIGUSR1, once every button is up
     * and every stick at rest (sticks only up to PENDING_LIMIT_MS). */
    int pending = 0;
    long long pending_since = 0;
    while (!stopping) {
        long long t = now_ms();
        if (t - last_scan >= RESCAN_MS) {
            rescan();
            last_scan = t;
        }
        int seconds = sleep_after_seconds(config);
        if (sleep_requested) {
            sleep_requested = 0;
            if (!pending) pending_since = t;
            pending = 1;
        }
        if (screen == SCREEN_OFF) pending = 0;
        long long off_at = last_use + (long long)seconds * 1000;
        long long dim_at = off_at - (seconds > DIM_SECONDS * 2 ? DIM_SECONDS * 1000 : 0);
        /* What is held is only read when the device is due to dim or sleep. */
        int held = seconds && screen != SCREEN_OFF && t >= dim_at && idle_held_counts(t, last_event) && controls_held();
        /* Off, after a failed sleep, waits for a press. */
        enum screen_state next = screen == SCREEN_OFF ? SCREEN_OFF : idle_next(screen, t, &last_use, seconds, DIM_SECONDS, held);
        int released = pending && !any_key_down() && (t - pending_since >= PENDING_LIMIT_MS || !controls_held());
        if ((next == SCREEN_OFF && screen != SCREEN_OFF) || released) {
            pending = 0;
            enum suspend_result r = suspend();
            if (r != SUSPEND_FAILED) {
                last_use = last_event = now_ms();
                /* A cancelled sleep did not resume, so nothing needs to settle. */
                if (r == SUSPEND_SLEPT) settle_until = last_use + SETTLE_MS;
                set_screen(SCREEN_ON);
            }
            continue;
        }
        set_screen(next);
        off_at = last_use + (long long)seconds * 1000;
        dim_at = off_at - (seconds > DIM_SECONDS * 2 ? DIM_SECONDS * 1000 : 0);

        long long wait = RESCAN_MS;
        if (seconds && screen == SCREEN_ON && dim_at - t < wait) wait = dim_at - t;
        if (seconds && screen == SCREEN_DIM && off_at - t < wait) wait = off_at - t;
        if (pending && PENDING_MS < wait) wait = PENDING_MS;
        if (grabbed && screen != SCREEN_OFF && t < settle_until && settle_until - t < wait) wait = settle_until - t;
        if (wait < 0) wait = 0;
        struct pollfd fds[MAX_DEVICES];
        for (int i = 0; i < ndevices; i++) fds[i] = (struct pollfd){.fd = devices[i].fd, .events = POLLIN};
        int n = poll(fds, ndevices, (int)wait);
        if (n < 0 && errno != EINTR) {
            perror("gizlet-power: poll");
            break;
        }

        int used = 0, pressed = 0, power = 0;
        for (int i = ndevices - 1; i >= 0; i--) {
            if (!(fds[i].revents & (POLLIN | POLLERR | POLLHUP))) continue;
            struct input_event ev[64];
            ssize_t len;
            while ((len = read(devices[i].fd, ev, sizeof ev)) > 0) {
                for (size_t k = 0; k < (size_t)len / sizeof ev[0]; k++) {
                    used |= is_activity(&devices[i], &ev[k], &pressed, &power);
                }
            }
            if (len < 0 && errno != EAGAIN && errno != EINTR) close_device(i);
        }
        t = now_ms();
        /* What woke the device is not use, nor a new press of the power key. */
        if (t < settle_until) used = pressed = power = 0;
        if (screen == SCREEN_OFF) {
            if (pressed) {
                last_use = last_event = t;
                set_screen(SCREEN_ON);
            }
        } else {
            if (used) {
                last_use = last_event = t;
                set_screen(SCREEN_ON);
            }
            if (power && !pending) {
                pending = 1;
                pending_since = t;
            }
        }
        /* The press that woke the device stays with us until it is let go. */
        if (grabbed && screen != SCREEN_OFF && t >= settle_until && !any_key_down()) grab(0);
    }
    if (grabbed) grab(0);
    set_screen(SCREEN_ON);
    return 0;
}
