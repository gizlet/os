/*
 * gizlet-audio-jack CONTROL HEADPHONE SPEAKER: routes sound to the speaker
 * or the headphone jack, following the jack.
 *
 * The R36S's RK817 codec plays to one output at a time, chosen by the mixer
 * enum CONTROL ("Playback Mux": HP or SPK), and nothing in the kernel switches
 * it. The sound card reports the jack as an input device with
 * SW_HEADPHONE_INSERT. This sets CONTROL to HEADPHONE while headphones are in
 * and to SPEAKER otherwise, at start and on every change, through amixer.
 * Without a jack device it selects the speaker and exits.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *control;

static void route(int headphones, const char *item) {
    pid_t pid = fork();
    if (pid == 0) {
        execlp("amixer", "amixer", "-q", "sset", control, item, (char *)NULL);
        _exit(127);
    }
    int status = 0;
    if (pid < 0 || waitpid(pid, &status, 0) < 0 || !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "gizlet-audio-jack: cannot set %s to %s\n", control, item);
        return;
    }
    printf("gizlet-audio-jack: %s, sound to %s\n", headphones ? "headphones in" : "no headphones", item);
    fflush(stdout);
}

static int test_bit(const unsigned char *bits, int n) {
    return (bits[n / 8] >> (n % 8)) & 1;
}

/* The first input device that reports SW_HEADPHONE_INSERT, or -1. */
static int open_jack(void) {
    DIR *dir = opendir("/dev/input");
    if (!dir) return -1;
    struct dirent *e;
    int found = -1;
    while (found < 0 && (e = readdir(dir))) {
        if (strncmp(e->d_name, "event", 5) != 0) continue;
        int fd = openat(dirfd(dir), e->d_name, O_RDONLY | O_CLOEXEC);
        if (fd < 0) continue;
        unsigned char sw[(SW_MAX + 8) / 8] = {0};
        if (ioctl(fd, EVIOCGBIT(EV_SW, sizeof sw), sw) >= 0 && test_bit(sw, SW_HEADPHONE_INSERT)) {
            found = fd;
        } else {
            close(fd);
        }
    }
    closedir(dir);
    return found;
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fputs("usage: gizlet-audio-jack CONTROL HEADPHONE_ITEM SPEAKER_ITEM\n", stderr);
        return 2;
    }
    control = argv[1];
    const char *items[2] = {argv[3], argv[2]};
    int fd = open_jack();
    if (fd < 0) {
        route(0, items[0]);
        return 0;
    }
    unsigned char sw[(SW_MAX + 8) / 8] = {0};
    int in = ioctl(fd, EVIOCGSW(sizeof sw), sw) >= 0 && test_bit(sw, SW_HEADPHONE_INSERT);
    route(in, items[in]);
    struct input_event ev;
    for (;;) {
        ssize_t n = read(fd, &ev, sizeof ev);
        if (n < 0 && errno == EINTR) continue;
        if (n != (ssize_t)sizeof ev) {
            perror("gizlet-audio-jack: read");
            return 1;
        }
        if (ev.type == EV_SW && ev.code == SW_HEADPHONE_INSERT && (ev.value != 0) != in) {
            in = ev.value != 0;
            route(in, items[in]);
        }
    }
}
