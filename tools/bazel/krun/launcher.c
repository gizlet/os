/* Runs one command in a libkrun microVM and exits with its status.
 *
 * krun-vm --krunfw LIB (--root DIR | --root-disk IMAGE) [--cpus N] [--mem MIB]
 *         [--share TAG=DIR]... [--workdir DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...
 *
 * The guest environment contains only the --env assignments. Shares are exposed
 * as virtio-fs tags; the guest command mounts them where it needs them.
 */
#include <dlfcn.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "libkrun.h"

#define MAX_ITEMS 64

static void usage(void) {
    fputs("usage: krun-vm --krunfw LIB (--root DIR | --root-disk IMAGE) [--cpus N] [--mem MIB]\n"
          "               [--share TAG=DIR]... [--workdir DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...\n",
          stderr);
    exit(2);
}

static void check(int32_t result, const char *what) {
    if (result < 0) {
        fprintf(stderr, "krun-vm: %s: %s\n", what, strerror(-result));
        exit(1);
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

int main(int argc, char **argv) {
    const char *krunfw = NULL, *root = NULL, *root_disk = NULL, *workdir = NULL;
    const char *shares[MAX_ITEMS];
    const char *envp[MAX_ITEMS + 1];
    size_t share_count = 0, env_count = 0;
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
        } else if (strcmp(arg, "--workdir") == 0) {
            workdir = value;
        } else if (strcmp(arg, "--share") == 0 && share_count < MAX_ITEMS && strchr(value, '=')) {
            shares[share_count++] = value;
        } else if (strcmp(arg, "--env") == 0 && env_count < MAX_ITEMS && strchr(value, '=')) {
            envp[env_count++] = value;
        } else {
            usage();
        }
    }
    envp[env_count] = NULL;
    if (!krunfw || (!root == !root_disk) || i >= argc) {
        usage();
    }

    /* libkrun opens libkrunfw by its install name. Loading the pinned build first
     * makes that lookup resolve to it instead of searching the host. */
    if (!dlopen(krunfw, RTLD_NOW | RTLD_GLOBAL)) {
        fprintf(stderr, "krun-vm: %s\n", dlerror());
        return 1;
    }

    check(krun_set_log_level(getenv("KRUN_VM_DEBUG") ? 3 : 1), "log level");
    int32_t ctx = krun_create_ctx();
    check(ctx, "create context");
    check(krun_set_vm_config((uint32_t)ctx, (uint8_t)cpus, (uint32_t)mem), "VM configuration");
    if (root) {
        check(krun_set_root((uint32_t)ctx, root), "root directory");
    } else {
        check(krun_add_disk((uint32_t)ctx, "root", root_disk, true), "root disk");
        check(krun_set_root_disk_remount((uint32_t)ctx, "/dev/vda", "ext4", "ro"), "root remount");
    }
    for (size_t s = 0; s < share_count; s++) {
        char tag[256];
        const char *eq = strchr(shares[s], '=');
        size_t length = (size_t)(eq - shares[s]);
        if (length == 0 || length >= sizeof tag) {
            usage();
        }
        memcpy(tag, shares[s], length);
        tag[length] = '\0';
        check(krun_add_virtiofs((uint32_t)ctx, tag, eq + 1), "share");
    }
    if (workdir) {
        check(krun_set_workdir((uint32_t)ctx, workdir), "working directory");
    }
    check(krun_set_exec((uint32_t)ctx, argv[i], (const char *const *)&argv[i + 1], envp), "command");
    check(krun_start_enter((uint32_t)ctx), "start");
    return 1;
}
