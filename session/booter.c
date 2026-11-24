#define _GNU_SOURCE
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define INSTALL_DIR "/opt/gizlet"
#define CONFIG_FILE "boot.ini"
#define MAX_CONFIG_SIZE 4096

/* Paths are relative to the mounted installation directory. Open each component
 * without following symlinks and execute through the resulting file descriptor. */
static int open_entry(int root, char *path) {
    int dir = dup(root);
    if (dir < 0)
        return -1;
    for (;;) {
        char *slash = strchr(path, '/');
        if (slash)
            *slash = '\0';
        bool valid = path[0] && strcmp(path, ".") != 0 && strcmp(path, "..") != 0;
        for (const unsigned char *c = (unsigned char *)path; valid && *c; c++) {
            valid = isalnum(*c) || strchr("._+-", *c) != NULL;
        }
        if (!valid) {
            close(dir);
            errno = EINVAL;
            return -1;
        }
        int next = openat(dir, path,
                          O_NOFOLLOW |
                              (slash ? O_DIRECTORY | O_RDONLY | O_CLOEXEC : O_RDONLY | O_NONBLOCK));
        int saved = errno;
        close(dir);
        if (next < 0) {
            errno = saved;
            return -1;
        }
        if (!slash)
            return next;
        dir = next;
        path = slash + 1;
    }
}

static bool read_config(int fd, char *buffer) {
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size > MAX_CONFIG_SIZE)
        return false;
    size_t used = 0;
    while (used <= MAX_CONFIG_SIZE) {
        ssize_t count = read(fd, buffer + used, MAX_CONFIG_SIZE + 1 - used);
        if (count < 0 && errno == EINTR)
            continue;
        if (count < 0)
            return false;
        if (count == 0) {
            if (memchr(buffer, '\0', used))
                return false;
            buffer[used] = '\0';
            return true;
        }
        used += (size_t)count;
    }
    return false;
}

static char *parse_entry(char *contents) {
    char *entry = NULL;
    bool section = false;
    char *line;
    while ((line = strsep(&contents, "\n")) != NULL) {
        size_t len = strlen(line);
        if (len && line[len - 1] == '\r')
            line[--len] = '\0';
        if (!len || line[0] == '#')
            continue;
        if (!section && strcmp(line, "[boot]") == 0) {
            section = true;
        } else if (section && !entry && strncmp(line, "entry=", 6) == 0 && line[6]) {
            entry = line + 6;
        } else {
            return NULL;
        }
    }
    return entry;
}

int main(void) {
    int root = open(INSTALL_DIR, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (root < 0 || fchdir(root) < 0) {
        perror("Gizlet OS: " INSTALL_DIR);
        return 1;
    }
    int config = openat(root, CONFIG_FILE, O_RDONLY | O_NONBLOCK | O_NOFOLLOW | O_CLOEXEC);
    if (config < 0) {
        perror("Gizlet OS: " INSTALL_DIR "/" CONFIG_FILE);
        return 1;
    }
    char contents[MAX_CONFIG_SIZE + 1];
    bool ok = read_config(config, contents);
    close(config);
    char *entry = ok ? parse_entry(contents) : NULL;
    if (!entry) {
        fputs("Gizlet OS: invalid boot.ini (expected [boot] and one entry)\n", stderr);
        return 1;
    }
    char path[MAX_CONFIG_SIZE + 1];
    strcpy(path, entry);
    int executable = open_entry(root, path);
    close(root);
    if (executable < 0) {
        perror("Gizlet OS: cannot open configured entry");
        return 1;
    }
    struct stat st;
    if (fstat(executable, &st) || !S_ISREG(st.st_mode) || !(st.st_mode & 0111)) {
        fputs("Gizlet OS: entry must be an executable regular file\n", stderr);
        return 1;
    }
    /* Keep this descriptor open through exec so a shebang interpreter opens
     * the same verified inode through procfs. The kernel chooses the binary format. */
    char fd_path[64];
    snprintf(fd_path, sizeof(fd_path), "/proc/self/fd/%d", executable);
    char *const argv[] = {entry, NULL};
    printf("Gizlet OS: starting %s/%s\n", INSTALL_DIR, entry);
    fflush(stdout);
    execv(fd_path, argv);
    perror("Gizlet OS: execute entry");
    return 1;
}
