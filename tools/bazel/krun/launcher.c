/* Runs one command in a libkrun microVM and exits with its status.
 *
 * krun-vm --krunfw LIB (--root DIR | --root-disk SQUASHFS) [--cpus N] [--mem MIB]
 *         [--inputs MANIFEST] [--outputs DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...
 *
 * MANIFEST lists "SOURCE<TAB>DESTINATION" lines. Each source is resolved on the
 * host and linked, or copied, into a private staging directory exposed to the
 * guest as the virtio-fs tag "in". Bazel's execroot is a symlink forest whose
 * absolute targets do not exist in the guest, so it is never shared directly.
 * DIR is exposed as the tag "out". The guest command mounts both tags.
 * The guest environment contains only the --env assignments.
 *
 * libkrun reports the guest's exit status only for a virtio-fs root. With
 * --inputs the guest instead writes its decimal status to /in/.krun-status,
 * and that value becomes this process's status. A missing status is a failure.
 */
#define _DARWIN_C_SOURCE
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <ftw.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#include "libkrun.h"

#define MAX_ENV 64

static void usage(void) {
    fputs("usage: krun-vm --krunfw LIB (--root DIR | --root-disk SQUASHFS) [--cpus N] [--mem MIB]\n"
          "               [--inputs MANIFEST] [--outputs DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...\n",
          stderr);
    exit(2);
}

static void die(const char *what, const char *detail) {
    fprintf(stderr, "krun-vm: %s%s%s: %s\n", what, detail ? " " : "", detail ? detail : "", strerror(errno));
    exit(1);
}

static void check(int32_t result, const char *what) {
    if (result < 0) {
        errno = -result;
        die(what, NULL);
    }
}

static unsigned long number(const char *text, unsigned long max) {
    char *end;
    errno = 0;
    unsigned long value = strtoul(text, &end, 10);
    if (errno || *text == '\0' || *end != '\0' || value == 0 || value > max) {
        usage();
    }
    return value;
}

static bool safe_destination(const char *path) {
    if (*path == '\0' || *path == '/') {
        return false;
    }
    for (const char *part = path; part;) {
        const char *slash = strchr(part, '/');
        size_t length = slash ? (size_t)(slash - part) : strlen(part);
        if (length == 0 || (length == 1 && part[0] == '.') || (length == 2 && part[0] == '.' && part[1] == '.')) {
            return false;
        }
        part = slash ? slash + 1 : NULL;
    }
    return true;
}

static void make_parents(char *path) {
    for (char *slash = strchr(path + 1, '/'); slash; slash = strchr(slash + 1, '/')) {
        *slash = '\0';
        if (mkdir(path, 0755) != 0 && errno != EEXIST) {
            die("mkdir", path);
        }
        *slash = '/';
    }
}

static void copy_file(const char *source, const char *target) {
    int in = open(source, O_RDONLY);
    if (in < 0) {
        die("open", source);
    }
    struct stat info;
    if (fstat(in, &info) != 0) {
        die("stat", source);
    }
    int out = open(target, O_WRONLY | O_CREAT | O_EXCL, info.st_mode & 0777);
    if (out < 0) {
        die("create", target);
    }
    char buffer[1 << 16];
    for (;;) {
        ssize_t count = read(in, buffer, sizeof buffer);
        if (count < 0) {
            die("read", source);
        }
        if (count == 0) {
            break;
        }
        for (ssize_t done = 0; done < count;) {
            ssize_t written = write(out, buffer + done, (size_t)(count - done));
            if (written < 0) {
                die("write", target);
            }
            done += written;
        }
    }
    close(in);
    if (close(out) != 0) {
        die("close", target);
    }
}

static void stage_inputs(const char *manifest, const char *staging) {
    FILE *file = fopen(manifest, "r");
    if (!file) {
        die("open", manifest);
    }
    char line[2 * PATH_MAX + 2];
    while (fgets(line, sizeof line, file)) {
        line[strcspn(line, "\n")] = '\0';
        char *tab = strchr(line, '\t');
        if (!tab || !safe_destination(tab + 1)) {
            fprintf(stderr, "krun-vm: invalid manifest line: %s\n", line);
            exit(2);
        }
        *tab = '\0';
        char source[PATH_MAX], target[PATH_MAX];
        if (!realpath(line, source)) {
            die("resolve", line);
        }
        if (snprintf(target, sizeof target, "%s/%s", staging, tab + 1) >= (int)sizeof target) {
            errno = ENAMETOOLONG;
            die("stage", tab + 1);
        }
        make_parents(target);
        if (link(source, target) != 0) {
            copy_file(source, target);
        }
    }
    fclose(file);
}

static int remove_entry(const char *path, const struct stat *info, int type, struct FTW *walk) {
    (void)info, (void)type, (void)walk;
    return remove(path);
}

static void add_share(int32_t ctx, const char *tag, const char *path) {
    char resolved[PATH_MAX];
    if (!realpath(path, resolved)) {
        die("resolve", path);
    }
    check(krun_add_virtiofs((uint32_t)ctx, tag, resolved), "share");
}

int main(int argc, char **argv) {
    const char *krunfw = NULL, *root = NULL, *root_disk = NULL, *inputs = NULL, *outputs = NULL;
    const char *envp[MAX_ENV + 1];
    size_t env_count = 0;
    unsigned long cpus = 2, mem = 2048;
    int i = 1;
    for (; i < argc; i++) {
        const char *arg = argv[i];
        if (strcmp(arg, "--") == 0) {
            i++;
            break;
        }
        if (i + 1 >= argc) {
            usage();
        }
        const char *value = argv[++i];
        if (strcmp(arg, "--krunfw") == 0) {
            krunfw = value;
        } else if (strcmp(arg, "--root") == 0) {
            root = value;
        } else if (strcmp(arg, "--root-disk") == 0) {
            root_disk = value;
        } else if (strcmp(arg, "--cpus") == 0) {
            cpus = number(value, 255);
        } else if (strcmp(arg, "--mem") == 0) {
            mem = number(value, UINT32_MAX);
        } else if (strcmp(arg, "--inputs") == 0) {
            inputs = value;
        } else if (strcmp(arg, "--outputs") == 0) {
            outputs = value;
        } else if (strcmp(arg, "--env") == 0 && env_count < MAX_ENV && strchr(value, '=')) {
            envp[env_count++] = value;
        } else {
            usage();
        }
    }
    envp[env_count] = NULL;
    if (!krunfw || (!root == !root_disk) || i >= argc || (inputs && !outputs)) {
        usage();
    }

    /* Stage next to the outputs, on the same filesystem as Bazel's output base,
     * so that most inputs are hard links rather than copies. */
    char staging[PATH_MAX] = "";
    if (inputs) {
        if (snprintf(staging, sizeof staging, "%s/.krun-in.XXXXXX", outputs) >= (int)sizeof staging) {
            usage();
        }
        if (!mkdtemp(staging)) {
            die("mkdtemp", staging);
        }
        stage_inputs(inputs, staging);
    }

    /* krun_start_enter exits the process when the guest finishes. Run the VM in
     * a child so this process can remove the staging tree and forward its status. */
    pid_t child = fork();
    if (child < 0) {
        die("fork", NULL);
    }
    if (child > 0) {
        int status;
        while (waitpid(child, &status, 0) < 0) {
            if (errno != EINTR) {
                die("wait", NULL);
            }
        }
        int code = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
        if (*staging) {
            char path[PATH_MAX];
            snprintf(path, sizeof path, "%s/.krun-status", staging);
            FILE *file = fopen(path, "r");
            int reported;
            if (!file || fscanf(file, "%d", &reported) != 1 || reported < 0 || reported > 255) {
                fprintf(stderr, "krun-vm: the guest reported no exit status (VM status %d)\n", code);
                code = code ? code : 1;
            } else {
                code = reported;
            }
            if (file) {
                fclose(file);
            }
            if (nftw(staging, remove_entry, 16, FTW_DEPTH | FTW_PHYS) != 0) {
                die("remove", staging);
            }
        }
        return code;
    }

    /* libkrun opens libkrunfw by its install name. Loading the pinned build first
     * makes that lookup resolve to it instead of searching the host. */
    if (!dlopen(krunfw, RTLD_NOW | RTLD_GLOBAL)) {
        fprintf(stderr, "krun-vm: %s\n", dlerror());
        _exit(1);
    }
    check(krun_set_log_level(getenv("KRUN_VM_DEBUG") ? 3 : 1), "log level");
    int32_t ctx = krun_create_ctx();
    check(ctx, "create context");
    check(krun_set_vm_config((uint32_t)ctx, (uint8_t)cpus, (uint32_t)mem), "VM configuration");
    if (root) {
        char resolved[PATH_MAX];
        if (!realpath(root, resolved)) {
            die("resolve", root);
        }
        check(krun_set_root((uint32_t)ctx, resolved), "root directory");
    } else {
        char resolved[PATH_MAX];
        if (!realpath(root_disk, resolved)) {
            die("resolve", root_disk);
        }
        check(krun_add_disk((uint32_t)ctx, "root", resolved, true), "root disk");
        check(krun_set_root_disk_remount((uint32_t)ctx, "/dev/vda", "squashfs", "ro"), "root remount");
    }
    if (*staging) {
        add_share(ctx, "in", staging);
    }
    if (outputs) {
        add_share(ctx, "out", outputs);
    }
    check(krun_set_exec((uint32_t)ctx, argv[i], (const char *const *)&argv[i + 1], envp), "command");
    check(krun_start_enter((uint32_t)ctx), "start");
    _exit(1);
}
