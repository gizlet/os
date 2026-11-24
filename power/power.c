/*
 * gizlet-power [CONFIG]: turns the screen off when the device is idle.
 *
 * Every input device with buttons or axes is read alongside its other
 * readers. After `screen_off` seconds without input (CONFIG, by default
 * /run/gizlet-data/system/power.ini, which the launcher writes; 0 is never,
 * 300 when unset) the backlight turns off; 15 s before that it dims.
 *
 * - Dimmed: any input brings the brightness back.
 * - Off: the input devices are grabbed, so the press that wakes the screen
 *   reaches no app. A button press turns the backlight on again; the grab is
 *   released once every button is up. Sticks do not wake it.
 *
 * It runs on the system, not in the launcher, so the timer also runs while an
 * app has the screen. The file is read again whenever it changes.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
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
#define DEFAULT_SCREEN_OFF 300
#define RESCAN_MS 2000
#define BITS(n) (((n) + 7) / 8)

struct device {
    int fd;
    char name[32];
    unsigned char abs[BITS(ABS_CNT)];
    struct input_absinfo info[ABS_CNT];
    int last[ABS_CNT];
    /* Each axis's value when the device was opened, where it rests. */
    int rest[ABS_CNT];
};

static struct device devices[MAX_DEVICES];
static int ndevices;
static char backlight[256];
enum state { ON, DIM, OFF };
static enum state screen = ON;
static int saved_brightness = -1;
static int grabbed;
static volatile sig_atomic_t stopping;

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

/* screen_off from CONFIG; reread only when the file changes. */
static int screen_off_seconds(const char *config) {
    static struct timespec seen = {-1, 0};
    static int seconds = DEFAULT_SCREEN_OFF;
    struct stat st;
    if (stat(config, &st) != 0) {
        seconds = DEFAULT_SCREEN_OFF;
        seen.tv_sec = -1;
        return seconds;
    }
    if (st.st_mtim.tv_sec == seen.tv_sec && st.st_mtim.tv_nsec == seen.tv_nsec) return seconds;
    seen = st.st_mtim;
    seconds = DEFAULT_SCREEN_OFF;
    FILE *f = fopen(config, "re");
    if (!f) return seconds;
    char line[128];
    while (fgets(line, sizeof line, f)) {
        int v;
        if (sscanf(line, " screen_off = %d", &v) == 1 && v >= 0) seconds = v;
    }
    fclose(f);
    printf("gizlet-power: screen off after %s%d s\n", seconds ? "" : "never, ", seconds);
    fflush(stdout);
    return seconds;
}

static void grab(int on) {
    for (int i = 0; i < ndevices; i++) ioctl(devices[i].fd, EVIOCGRAB, on);
    grabbed = on;
}

static void set_screen(enum state next) {
    if (next == screen) return;
    if (screen == ON) saved_brightness = read_int(backlight, "brightness");
    int max = read_int(backlight, "max_brightness");
    if (next == ON) {
        write_int(backlight, "bl_power", 0);
        if (saved_brightness > 0) write_int(backlight, "brightness", saved_brightness);
    } else if (next == DIM) {
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
        int fd = openat(dirfd(dir), e->d_name, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
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
                    d->last[a] = d->rest[a] = d->info[a].value;
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
 * by an eighth of its range since the last use, so sensor noise does not. */
static int is_activity(struct device *d, const struct input_event *ev, int *pressed) {
    if (ev->type == EV_KEY) {
        if (ev->value == 1) *pressed = 1;
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
    (void)sig;
    stopping = 1;
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

    long long last_use = now_ms(), last_scan = 0;
    while (!stopping) {
        long long t = now_ms();
        if (t - last_scan >= RESCAN_MS) {
            rescan();
            last_scan = t;
        }
        int seconds = screen_off_seconds(config);
        long long off_at = last_use + (long long)seconds * 1000;
        long long dim_at = off_at - (seconds > DIM_SECONDS * 2 ? DIM_SECONDS * 1000 : 0);
        /* Due to dim or turn off: a control held all along is use too. */
        if (seconds && screen != OFF && t >= dim_at && controls_held()) {
            last_use = t;
            off_at = last_use + (long long)seconds * 1000;
            dim_at = off_at - (seconds > DIM_SECONDS * 2 ? DIM_SECONDS * 1000 : 0);
            if (screen == DIM) set_screen(ON);
        }
        if (seconds == 0) {
            if (screen != ON) set_screen(ON);
        } else if (t >= off_at) {
            set_screen(OFF);
        } else if (t >= dim_at && screen == ON) {
            set_screen(DIM);
        }

        long long wait = RESCAN_MS;
        if (seconds && screen == ON && dim_at - t < wait) wait = dim_at - t;
        if (seconds && screen == DIM && off_at - t < wait) wait = off_at - t;
        if (wait < 0) wait = 0;
        struct pollfd fds[MAX_DEVICES];
        for (int i = 0; i < ndevices; i++) fds[i] = (struct pollfd){.fd = devices[i].fd, .events = POLLIN};
        int n = poll(fds, ndevices, (int)wait);
        if (n < 0 && errno != EINTR) {
            perror("gizlet-power: poll");
            break;
        }

        int used = 0, pressed = 0;
        for (int i = ndevices - 1; i >= 0; i--) {
            if (!(fds[i].revents & (POLLIN | POLLERR | POLLHUP))) continue;
            struct input_event ev[64];
            ssize_t len;
            while ((len = read(devices[i].fd, ev, sizeof ev)) > 0) {
                for (size_t k = 0; k < (size_t)len / sizeof ev[0]; k++) used |= is_activity(&devices[i], &ev[k], &pressed);
            }
            if (len < 0 && errno != EAGAIN && errno != EINTR) close_device(i);
        }
        if (screen == OFF) {
            if (pressed) {
                last_use = now_ms();
                set_screen(ON);
            }
        } else if (used) {
            last_use = now_ms();
            set_screen(ON);
        }
        /* The press that woke the screen stays with us until it is let go. */
        if (grabbed && screen != OFF && !any_key_down()) grab(0);
    }
    if (grabbed) grab(0);
    set_screen(ON);
    return 0;
}
