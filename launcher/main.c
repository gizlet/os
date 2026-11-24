// Gizlet launcher, first version: a button test.
//
// It lists the input devices and prints every button press and release, and
// large stick movements, on its console and on the LCD (tty1). Start+Select
// together, or a line starting with q on standard input, ends it.
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>

#define MAX_DEVICES 16
#define STICK_STEP 256

struct device {
    int fd;
    char name[64];
    int axis[ABS_CNT];
};

static struct device devices[MAX_DEVICES];
static int device_count;
static bool held[KEY_CNT];
static FILE *lcd;

static void say(const char *format, ...) {
    va_list args;
    va_start(args, format);
    vprintf(format, args);
    va_end(args);
    fflush(stdout);
    if (lcd) {
        va_start(args, format);
        vfprintf(lcd, format, args);
        va_end(args);
        fflush(lcd);
    }
}

// The LCD console, when it is not already standard output.
static FILE *open_lcd(void) {
    struct stat out, tty;
    int fd = open("/dev/tty1", O_WRONLY | O_NOCTTY | O_CLOEXEC);
    if (fd < 0) {
        return NULL;
    }
    if (fstat(STDOUT_FILENO, &out) == 0 && fstat(fd, &tty) == 0 && S_ISCHR(out.st_mode) && out.st_rdev == tty.st_rdev) {
        close(fd);
        return NULL;
    }
    return fdopen(fd, "w");
}

static const char *key_name(unsigned code) {
    static const struct {
        unsigned code;
        const char *name;
    } names[] = {
        {BTN_SOUTH, "B (south)"},
        {BTN_EAST, "A (east)"},
        {BTN_NORTH, "X (north)"},
        {BTN_WEST, "Y (west)"},
        {BTN_TL, "L1"},
        {BTN_TR, "R1"},
        {BTN_TL2, "L2"},
        {BTN_TR2, "R2"},
        {BTN_SELECT, "Select"},
        {BTN_START, "Start"},
        {BTN_MODE, "Mode"},
        {BTN_THUMBL, "L3"},
        {BTN_THUMBR, "R3"},
        {BTN_DPAD_UP, "D-pad up"},
        {BTN_DPAD_DOWN, "D-pad down"},
        {BTN_DPAD_LEFT, "D-pad left"},
        {BTN_DPAD_RIGHT, "D-pad right"},
        {KEY_VOLUMEUP, "Volume up"},
        {KEY_VOLUMEDOWN, "Volume down"},
        {KEY_POWER, "Power"},
        {KEY_UP, "Up"},
        {KEY_DOWN, "Down"},
        {KEY_LEFT, "Left"},
        {KEY_RIGHT, "Right"},
        {KEY_ENTER, "Enter"},
        {KEY_ESC, "Esc"},
        {KEY_SPACE, "Space"},
    };
    static char other[24];
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        if (names[i].code == code) {
            return names[i].name;
        }
    }
    snprintf(other, sizeof other, "key 0x%03x", code);
    return other;
}

static const char *axis_name(unsigned code) {
    switch (code) {
    case ABS_X: return "left stick X";
    case ABS_Y: return "left stick Y";
    case ABS_RX: return "right stick X";
    case ABS_RY: return "right stick Y";
    case ABS_HAT0X: return "hat X";
    case ABS_HAT0Y: return "hat Y";
    default: return "axis";
    }
}

static void open_devices(void) {
    DIR *dir = opendir("/dev/input");
    struct dirent *entry;
    while (dir && (entry = readdir(dir)) && device_count < MAX_DEVICES) {
        if (strncmp(entry->d_name, "event", 5) != 0) {
            continue;
        }
        char path[300];
        snprintf(path, sizeof path, "/dev/input/%s", entry->d_name);
        struct device *device = &devices[device_count];
        device->fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (device->fd < 0) {
            continue;
        }
        if (ioctl(device->fd, EVIOCGNAME(sizeof device->name), device->name) < 0) {
            snprintf(device->name, sizeof device->name, "%.*s", (int)sizeof device->name - 1, entry->d_name);
        }
        for (unsigned axis = 0; axis < ABS_CNT; axis++) {
            struct input_absinfo info;
            device->axis[axis] = ioctl(device->fd, EVIOCGABS(axis), &info) == 0 ? info.value : 0;
        }
        say("  %s (%s)\n", device->name, path);
        device_count++;
    }
    if (dir) {
        closedir(dir);
    }
}

// Returns false when the player asked to leave.
static bool handle(struct device *device, const struct input_event *event) {
    if (event->type == EV_KEY && event->code < KEY_CNT && event->value != 2) {
        held[event->code] = event->value == 1;
        say("%-28s %-12s %s\n", device->name, key_name(event->code), event->value ? "pressed" : "released");
        return !(held[BTN_START] && held[BTN_SELECT]);
    }
    if (event->type == EV_ABS && event->code < ABS_CNT) {
        int last = device->axis[event->code];
        bool hat = event->code >= ABS_HAT0X && event->code <= ABS_HAT3Y;
        if (hat || abs(event->value - last) >= STICK_STEP) {
            device->axis[event->code] = event->value;
            say("%-28s %-12s %d\n", device->name, axis_name(event->code), event->value);
        }
    }
    return true;
}

int main(void) {
    lcd = open_lcd();
    if (lcd) {
        fputs("\033[H\033[2J", lcd);
    }
    say("Gizlet launcher: button test\n");
    say("Press Start and Select together to exit.\n\nInput devices:\n");
    open_devices();
    if (device_count == 0) {
        say("  none found\n");
    }
    say("\n");

    struct pollfd fds[MAX_DEVICES + 1];
    for (int i = 0; i < device_count; i++) {
        fds[i] = (struct pollfd){.fd = devices[i].fd, .events = POLLIN};
    }
    fds[device_count] = (struct pollfd){.fd = STDIN_FILENO, .events = POLLIN};
    for (;;) {
        if (poll(fds, device_count + 1, -1) < 0) {
            perror("launcher: poll");
            return 1;
        }
        for (int i = 0; i < device_count; i++) {
            if (fds[i].revents & (POLLERR | POLLHUP)) {
                say("%s disconnected\n", devices[i].name);
                fds[i].fd = -1;
                continue;
            }
            struct input_event events[32];
            ssize_t n;
            while ((fds[i].revents & POLLIN) && (n = read(devices[i].fd, events, sizeof events)) > 0) {
                for (size_t e = 0; e < (size_t)n / sizeof events[0]; e++) {
                    if (!handle(&devices[i], &events[e])) {
                        say("Leaving the launcher.\n");
                        return 0;
                    }
                }
            }
        }
        if (fds[device_count].revents & POLLIN) {
            char line[64];
            ssize_t n = read(STDIN_FILENO, line, sizeof line);
            if (n <= 0 || line[0] == 'q') {
                say("Leaving the launcher.\n");
                return 0;
            }
        } else if (fds[device_count].revents & (POLLHUP | POLLERR)) {
            fds[device_count].fd = -1;
        }
    }
}
