/*
 * krun-vm for AArch64 Linux hosts: runs one command in the build root inside
 * Linux namespaces instead of a libkrun microVM, with the same command line as
 * tools/bazel/krun/vm/main.rs:
 *
 *   krun-vm --krunfw LIB (--root DIR | --root-disk SQUASHFS) [--cpus N] [--mem MIB]
 *           [--inputs MANIFEST] [--outputs DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...
 *
 * The host is already AArch64 Linux, so the stages need the pinned root, not a
 * VM. The command runs as root of a new user namespace, with its own mount,
 * PID, network, IPC and UTS namespaces, in the root it is given:
 *
 * - The SquashFS root is unpacked once with the host's unsquashfs, next to the
 *   image and keyed by its inode, as Linux does not mount SquashFS in a user
 *   namespace. --krunfw, --cpus and --mem are accepted and ignored.
 * - /in holds the staged inputs, /out is DIR, /tmp is a scratch directory on
 *   disk next to DIR, /dev holds a few host devices and a private devpts,
 *   /proc and /sys are the host's. The root itself is read-only.
 * - KRUN_BACKEND=namespace tells the guest scripts that /in, /out and /tmp are
 *   already mounted (see defs.bzl).
 *
 * Packages keep their file owners (mmdebstrap's root mode chowns), so the
 * namespace maps 65536 ids:
 *
 * - As root: the host's own 0..65535.
 * - Otherwise the caller as 0 and the caller's /etc/subuid and /etc/subgid
 *   ranges as the rest, through newuidmap and newgidmap. In a container the
 *   kernel refuses that map for a namespace a normal user creates, so then
 *   krun-vm runs itself again with `sudo -n` (passwordless, as for a CI
 *   runner's user): root creates the namespace and writes the same map, and
 *   everything it leaves behind belongs to the caller.
 *   Without a range for the caller, root maps 100000 and up.
 * - Without newuidmap ranges or sudo only the caller is mapped, and chown to
 *   other IDs fails.
 *
 * The status comes from /in/.krun-status, written by the guest script, as with
 * the VM. A missing status is a failure.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <ftw.h>
#include <grp.h>
#include <limits.h>
#include <net/if.h>
#include <pwd.h>
#include <sched.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/syscall.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <unistd.h>

#define IDS 65536
/* The IDs after the caller's when root maps a user without /etc/subuid. */
#define DEFAULT_SUB 100000UL
#define MAX_ENV 64

static const char *root_dir, *root_disk, *inputs, *outputs;
/* --as UID:GID: run for this caller, after the sudo re-run. */
static int as_set;
static uid_t as_uid;
static gid_t as_gid;
static char **original_argv;
static const char *extra_env[MAX_ENV];
static int nenv;
static char **command;
static char staging[PATH_MAX], scratch[PATH_MAX], root[PATH_MAX];
static int to_parent[2], to_child[2];

static void die(const char *fmt, ...);

/* snprintf that refuses to truncate. */
__attribute__((format(printf, 3, 4))) static void fmt(char *buf, size_t size, const char *format, ...) {
    va_list ap;
    va_start(ap, format);
    int n = vsnprintf(buf, size, format, ap);
    va_end(ap);
    if (n < 0 || (size_t)n >= size) {
        errno = 0;
        die("path too long: %s", buf);
    }
}

static void die(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    fputs("krun-vm: ", stderr);
    vfprintf(stderr, fmt, ap);
    if (errno) fprintf(stderr, ": %s", strerror(errno));
    fputc('\n', stderr);
    va_end(ap);
    exit(1);
}

static void usage(void) {
    fputs("usage: krun-vm --krunfw LIB (--root DIR | --root-disk SQUASHFS) [--cpus N] [--mem MIB]\n"
          "               [--inputs MANIFEST] [--outputs DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...\n",
          stderr);
    exit(2);
}

static void parse(int argc, char **argv) {
    int i;
    for (i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--") == 0) {
            command = argv + i + 1;
            break;
        }
        if (i + 1 >= argc) usage();
        const char *opt = argv[i], *value = argv[++i];
        if (strcmp(opt, "--root") == 0) root_dir = value;
        else if (strcmp(opt, "--root-disk") == 0) root_disk = value;
        else if (strcmp(opt, "--inputs") == 0) inputs = value;
        else if (strcmp(opt, "--outputs") == 0) outputs = value;
        else if (strcmp(opt, "--env") == 0 && strchr(value, '=') && nenv < MAX_ENV) extra_env[nenv++] = value;
        else if (strcmp(opt, "--as") == 0) {
            unsigned u, g;
            if (sscanf(value, "%u:%u", &u, &g) != 2 || geteuid() != 0) usage();
            as_set = 1, as_uid = u, as_gid = g;
        }
        else if (strcmp(opt, "--krunfw") && strcmp(opt, "--cpus") && strcmp(opt, "--mem")) usage();
    }
    if (!command || !*command || !root_dir == !root_disk || !outputs || (inputs && !outputs)) usage();
}

/* mkdir -p for the directories leading to PATH. */
static void make_parents(const char *path) {
    char buf[PATH_MAX];
    fmt(buf, sizeof buf, "%s", path);
    for (char *p = buf + 1; (p = strchr(p, '/')); p++) {
        *p = 0;
        if (mkdir(buf, 0755) && errno != EEXIST) die("mkdir %s", buf);
        *p = '/';
    }
    errno = 0;
}

static int safe_destination(const char *d) {
    if (!*d || d[0] == '/' || strstr(d, "//") || d[strlen(d) - 1] == '/') return 0;
    char buf[PATH_MAX];
    fmt(buf, sizeof buf, "%s", d);
    for (char *part = strtok(buf, "/"); part; part = strtok(NULL, "/"))
        if (!strcmp(part, ".") || !strcmp(part, "..")) return 0;
    return 1;
}

static void copy_file(const char *from, const char *to) {
    int in = open(from, O_RDONLY | O_CLOEXEC);
    struct stat st;
    if (in < 0 || fstat(in, &st)) die("open %s", from);
    int out = open(to, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, st.st_mode & 07777);
    if (out < 0) die("create %s", to);
    char buf[1 << 16];
    ssize_t n;
    while ((n = read(in, buf, sizeof buf)) > 0)
        if (write(out, buf, n) != n) die("write %s", to);
    if (n < 0) die("read %s", from);
    close(in);
    close(out);
}

/* Each "SOURCE<TAB>DESTINATION" line: the resolved source linked, or copied, into the staging tree. */
static void stage_inputs(void) {
    FILE *f = fopen(inputs, "re");
    if (!f) die("open %s", inputs);
    char line[2 * PATH_MAX];
    while (fgets(line, sizeof line, f)) {
        line[strcspn(line, "\n")] = 0;
        char *tab = strchr(line, '\t');
        if (!tab || !safe_destination(tab + 1)) {
            errno = 0;
            fprintf(stderr, "krun-vm: invalid manifest line: %s\n", line);
            exit(2);
        }
        *tab = 0;
        char source[PATH_MAX], target[PATH_MAX];
        if (!realpath(line, source)) die("resolve %s", line);
        fmt(target, sizeof target, "%s/%s", staging, tab + 1);
        make_parents(target);
        if (link(source, target)) copy_file(source, target);
    }
    fclose(f);
    errno = 0;
}

static int remove_entry(const char *path, const struct stat *st, int type, struct FTW *ftw) {
    (void)st;
    (void)type;
    if (ftw->level > 0) remove(path);
    return 0;
}

/* Removes what is inside PATH, without following links. */
static void empty_tree(const char *path) {
    nftw(path, remove_entry, 64, FTW_DEPTH | FTW_PHYS | FTW_MOUNT);
}

static void remove_tree(const char *path) {
    empty_tree(path);
    rmdir(path);
}

static int chown_entry(const char *path, const struct stat *st, int type, struct FTW *ftw) {
    (void)st;
    (void)type;
    (void)ftw;
    if (lchown(path, as_uid, as_gid)) die("chown %s", path);
    return 0;
}

/* After the sudo re-run, what is made here belongs to the caller. */
static void give_to_caller(const char *path) {
    if (as_set) nftw(path, chown_entry, 64, FTW_PHYS | FTW_MOUNT);
    errno = 0;
}

static int run(char *const argv[]) {
    pid_t pid = fork();
    if (pid < 0) die("fork");
    if (pid == 0) {
        execvp(argv[0], argv);
        fprintf(stderr, "krun-vm: %s: %s\n", argv[0], strerror(errno));
        _exit(127);
    }
    int status;
    while (waitpid(pid, &status, 0) < 0)
        if (errno != EINTR) die("wait");
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}

/* The unpacked SquashFS root beside the image, made once per image file. */
static void unpack_root_disk(void) {
    char image[PATH_MAX], dir[PATH_MAX], lock[PATH_MAX], partial[PATH_MAX], name[NAME_MAX + 1];
    struct stat st;
    if (!realpath(root_disk, image) || stat(image, &st)) die("resolve %s", root_disk);
    fmt(dir, sizeof dir, "%s", image);
    char *slash = strrchr(dir, '/');
    *slash = 0;
    const char *base = slash + 1;
    char prefix[NAME_MAX + 1];
    fmt(prefix, sizeof prefix, ".%s.", base);
    fmt(name, sizeof name, "%s%lx-%lx-%llx.root", prefix, (unsigned long)st.st_dev, (unsigned long)st.st_ino,
             (unsigned long long)st.st_mtim.tv_sec * 1000000000ULL + (unsigned long long)st.st_mtim.tv_nsec);
    fmt(root, sizeof root, "%s/%s", dir, name);
    fmt(lock, sizeof lock, "%s/%slock", dir, prefix);
    int fd = open(lock, O_RDWR | O_CREAT | O_CLOEXEC, 0644);
    if (fd < 0 || flock(fd, LOCK_EX)) die("lock %s", lock);
    if (access(root, F_OK) != 0) {
        /* Roots of earlier builds of the image are not used again. */
        DIR *d = opendir(dir);
        struct dirent *e;
        while (d && (e = readdir(d))) {
            size_t n = strlen(e->d_name);
            if (strncmp(e->d_name, prefix, strlen(prefix)) || strcmp(e->d_name, name) == 0) continue;
            if ((n > 5 && !strcmp(e->d_name + n - 5, ".root")) || (n > 13 && !strcmp(e->d_name + n - 13, ".root.partial"))) {
                char old[PATH_MAX];
                fmt(old, sizeof old, "%s/%s", dir, e->d_name);
                remove_tree(old);
            }
        }
        if (d) closedir(d);
        fmt(partial, sizeof partial, "%s.partial", root);
        char *argv[] = {"unsquashfs", "-quiet", "-no-progress", "-no-xattrs", "-d", partial, image, NULL};
        fprintf(stderr, "krun-vm: unpacking %s for namespace builds\n", base);
        int status = run(argv);
        errno = 0;
        if (status == 127) die("unsquashfs is missing; install squashfs-tools");
        if (status) die("unsquashfs %s failed with status %d", image, status);
        give_to_caller(partial);
        if (rename(partial, root)) die("rename %s", partial);
    }
    if (as_set && fchown(fd, as_uid, as_gid)) die("chown %s", lock);
    close(fd);
    errno = 0;
}

static int write_file(const char *path, const char *text) {
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t n = write(fd, text, strlen(text));
    int saved = errno;
    close(fd);
    errno = saved;
    return n == (ssize_t)strlen(text) ? 0 : -1;
}

/* The first ID and count of NAME's or UID's range in /etc/subuid or /etc/subgid. */
static int sub_range(const char *file, const char *name, unsigned uid, unsigned long *start, unsigned long *count) {
    FILE *f = fopen(file, "re");
    if (!f) return 0;
    char line[256], who[128];
    int found = 0;
    while (!found && fgets(line, sizeof line, f)) {
        if (sscanf(line, "%127[^:]:%lu:%lu", who, start, count) != 3) continue;
        char id[16];
        fmt(id, sizeof id, "%u", uid);
        found = (name && !strcmp(who, name)) || !strcmp(who, id);
    }
    fclose(f);
    return found && *count > 0;
}

static int write_map(pid_t pid, const char *which, const char *map) {
    char path[64];
    fmt(path, sizeof path, "/proc/%d/%s", pid, which);
    return write_file(path, map);
}

/* Maps the IDs of process PID's new user namespace: 0 if done, -1 if refused. */
static int map_ids(pid_t pid) {
    char map[128];
    if (geteuid() == 0 && !as_set) {
        fmt(map, sizeof map, "0 0 %d\n", IDS);
        return write_map(pid, "uid_map", map) || write_map(pid, "gid_map", map) ? -1 : 0;
    }
    uid_t uid = as_set ? as_uid : geteuid();
    gid_t gid = as_set ? as_gid : getegid();
    struct passwd *pw = getpwuid(uid);
    const char *name = pw ? pw->pw_name : NULL;
    unsigned long us, uc, gs, gc;
    int ranges = sub_range("/etc/subuid", name, uid, &us, &uc) && sub_range("/etc/subgid", name, uid, &gs, &gc);
    if (uc > IDS - 1) uc = IDS - 1;
    if (gc > IDS - 1) gc = IDS - 1;
    if (as_set) {
        /* Root may map any range; /etc/subuid only keeps users apart. */
        if (!ranges) us = gs = DEFAULT_SUB, uc = gc = IDS - 1;
        fmt(map, sizeof map, "0 %u 1\n1 %lu %lu\n", (unsigned)uid, us, uc);
        if (write_map(pid, "uid_map", map)) return -1;
        fmt(map, sizeof map, "0 %u 1\n1 %lu %lu\n", (unsigned)gid, gs, gc);
        return write_map(pid, "gid_map", map) ? -1 : 0;
    }
    if (ranges) {
        char p[16], u[16], g[16], ustart[24], ucount[24], gstart[24], gcount[24];
        fmt(p, sizeof p, "%d", pid);
        fmt(u, sizeof u, "%u", (unsigned)uid);
        fmt(g, sizeof g, "%u", (unsigned)gid);
        fmt(ustart, sizeof ustart, "%lu", us);
        fmt(ucount, sizeof ucount, "%lu", uc);
        fmt(gstart, sizeof gstart, "%lu", gs);
        fmt(gcount, sizeof gcount, "%lu", gc);
        char *nu[] = {"newuidmap", p, "0", u, "1", "1", ustart, ucount, NULL};
        char *ng[] = {"newgidmap", p, "0", g, "1", "1", gstart, gcount, NULL};
        if (run(nu) == 0 && run(ng) == 0) return 0;
        return -1;
    }
    return -1;
}

/* Only the caller, as root of the namespace. */
static void map_caller_only(pid_t pid) {
    char map[64];
    fputs("krun-vm: no subordinate ID map and no passwordless sudo; only the caller is mapped,"
          " so stages that chown files will fail\n", stderr);
    write_map(pid, "setgroups", "deny");
    fmt(map, sizeof map, "0 %u 1\n", (unsigned)geteuid());
    if (write_map(pid, "uid_map", map)) die("write uid_map");
    fmt(map, sizeof map, "0 %u 1\n", (unsigned)getegid());
    if (write_map(pid, "gid_map", map)) die("write gid_map");
}

/* Runs this program again as root through passwordless sudo, for the caller. */
static void rerun_with_sudo(void) {
    /* sudo's own message says why it refuses, such as "no new privileges". */
    char *probe[] = {"sudo", "-n", "true", NULL};
    if (run(probe) != 0) {
        char line[64];
        FILE *f = fopen("/proc/self/status", "re");
        while (f && fgets(line, sizeof line, f))
            if (!strncmp(line, "NoNewPrivs:", 11)) fprintf(stderr, "krun-vm: sudo -n refused; %s", line);
        if (f) fclose(f);
        errno = 0;
        return;
    }
    char self[PATH_MAX], as[48];
    ssize_t n = readlink("/proc/self/exe", self, sizeof self - 1);
    if (n <= 0) return;
    self[n] = 0;
    fmt(as, sizeof as, "%u:%u", (unsigned)geteuid(), (unsigned)getegid());
    int argc = 0;
    while (original_argv[argc]) argc++;
    char **argv = calloc(argc + 6, sizeof *argv);
    int k = 0;
    argv[k++] = "sudo";
    argv[k++] = "-n";
    argv[k++] = self;
    argv[k++] = "--as";
    argv[k++] = as;
    for (int i = 1; i < argc; i++) argv[k++] = original_argv[i];
    /* The re-run stages its own inputs. */
    remove_tree(staging);
    remove_tree(scratch);
    execvp("sudo", argv);
    errno = 0;
}

static void bind_mount(const char *from, const char *to, unsigned long flags) {
    if (mount(from, to, NULL, MS_BIND | flags, NULL)) die("bind %s on %s", from, to);
}

static void at(char *buf, const char *path) {
    fmt(buf, PATH_MAX, "%s%s", root, path);
}

static void make_dev(void) {
    char p[PATH_MAX], q[PATH_MAX];
    at(p, "/dev");
    if (mount("dev", p, "tmpfs", MS_NOSUID | MS_NOEXEC, "mode=0755,size=64k")) die("mount %s", p);
    static const char *nodes[] = {"null", "zero", "full", "random", "urandom", "tty"};
    for (size_t i = 0; i < sizeof nodes / sizeof *nodes; i++) {
        char host[64];
        fmt(host, sizeof host, "/dev/%s", nodes[i]);
        fmt(q, sizeof q, "%s/%s", p, nodes[i]);
        int fd = open(q, O_CREAT | O_WRONLY | O_CLOEXEC, 0666);
        if (fd < 0) die("create %s", q);
        close(fd);
        if (mount(host, q, NULL, MS_BIND, NULL) && strcmp(nodes[i], "tty")) die("bind %s", host);
    }
    errno = 0;
    fmt(q, sizeof q, "%s/pts", p);
    mkdir(q, 0755);
    if (mount("devpts", q, "devpts", MS_NOSUID | MS_NOEXEC, "newinstance,ptmxmode=0666,mode=0620")) die("mount %s", q);
    fmt(q, sizeof q, "%s/shm", p);
    mkdir(q, 01777);
    if (mount("shm", q, "tmpfs", MS_NOSUID | MS_NODEV, "mode=1777")) die("mount %s", q);
    static const char *links[][2] = {
        {"ptmx", "pts/ptmx"}, {"fd", "/proc/self/fd"}, {"stdin", "/proc/self/fd/0"},
        {"stdout", "/proc/self/fd/1"}, {"stderr", "/proc/self/fd/2"},
    };
    for (size_t i = 0; i < sizeof links / sizeof *links; i++) {
        fmt(q, sizeof q, "%s/%s", p, links[i][0]);
        if (symlink(links[i][1], q)) die("link %s", q);
    }
}

/* The flags a read-only remount must keep: those locked on the source mount. */
static unsigned long locked_flags(const char *path) {
    struct statvfs sv;
    unsigned long flags = 0;
    if (statvfs(path, &sv) == 0) {
        if (sv.f_flag & ST_NOSUID) flags |= MS_NOSUID;
        if (sv.f_flag & ST_NODEV) flags |= MS_NODEV;
        if (sv.f_flag & ST_NOEXEC) flags |= MS_NOEXEC;
        if (sv.f_flag & ST_NOATIME) flags |= MS_NOATIME;
        if (sv.f_flag & ST_NODIRATIME) flags |= MS_NODIRATIME;
        if (sv.f_flag & ST_RELATIME) flags |= MS_RELATIME;
    }
    return flags;
}

static void loopback_up(void) {
    int s = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    struct ifreq ifr = {0};
    fmt(ifr.ifr_name, IFNAMSIZ, "lo");
    if (s < 0 || ioctl(s, SIOCGIFFLAGS, &ifr)) die("loopback");
    ifr.ifr_flags |= IFF_UP | IFF_RUNNING;
    if (ioctl(s, SIOCSIFFLAGS, &ifr)) die("loopback up");
    close(s);
}

/* PID 1 of the new namespaces: builds the root, runs the command, reaps. */
static int guest(void *unused) {
    (void)unused;
    char go;
    prctl(PR_SET_PDEATHSIG, SIGKILL);
    close(to_parent[0]);
    close(to_child[1]);
    if (write(to_parent[1], "u", 1) != 1 || read(to_child[0], &go, 1) != 1) _exit(125);
    if (setresgid(0, 0, 0) || setresuid(0, 0, 0)) die("become root of the namespace");
    setgroups(0, NULL);
    errno = 0;

    if (mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL)) die("make mounts private");
    bind_mount(root, root, MS_REC);
    char p[PATH_MAX];
    at(p, "/in");
    bind_mount(staging, p, 0);
    at(p, "/out");
    bind_mount(outputs, p, 0);
    at(p, "/tmp");
    bind_mount(scratch, p, 0);
    make_dev();
    at(p, "/proc");
    /* A container's /proc has covered paths, so a new proc is refused there; its own one is bound instead. */
    if (mount("proc", p, "proc", MS_NOSUID | MS_NODEV | MS_NOEXEC, NULL)) bind_mount("/proc", p, MS_REC);
    errno = 0;
    at(p, "/sys");
    bind_mount("/sys", p, MS_REC);
    if (mount(NULL, root, NULL, MS_REMOUNT | MS_BIND | MS_RDONLY | locked_flags(root), NULL))
        fprintf(stderr, "krun-vm: the root stays writable: %s\n", strerror(errno));
    errno = 0;
    if (chdir(root) || syscall(SYS_pivot_root, ".", ".") || umount2(".", MNT_DETACH) || chdir("/"))
        die("pivot to %s", root);
    loopback_up();
    if (sethostname("krun", 4)) errno = 0;

    pid_t worker = fork();
    if (worker < 0) die("fork");
    if (worker == 0) {
        char *env[MAX_ENV + 8];
        int n = 0;
        env[n++] = "PATH=/usr/sbin:/usr/bin:/sbin:/bin";
        env[n++] = "HOME=/";
        env[n++] = "TERM=linux";
        env[n++] = "KRUN_BACKEND=namespace";
        for (int i = 0; i < nenv; i++) env[n++] = (char *)extra_env[i];
        env[n] = NULL;
        execve(command[0], command, env);
        fprintf(stderr, "krun-vm: %s: %s\n", command[0], strerror(errno));
        _exit(127);
    }
    int status = 0, code;
    for (;;) {
        pid_t pid = wait(&status);
        if (pid == worker) break;
        if (pid < 0 && errno != EINTR) die("wait");
    }
    code = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
    /* The scratch tree may hold files of mapped IDs the caller cannot remove. */
    empty_tree("/tmp");
    return code;
}

static char guest_stack[1 << 20];

/* Starts the guest in new namespaces and maps its IDs; -1 when the map is
 * refused, unless CALLER_ONLY, which maps just the caller. */
static pid_t start_guest(int caller_only) {
    if (pipe2(to_parent, O_CLOEXEC) || pipe2(to_child, O_CLOEXEC)) die("pipe");
    int flags = CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS | SIGCHLD;
    pid_t child = clone(guest, guest_stack + sizeof guest_stack, flags, NULL);
    if (child < 0) die("create the namespaces (are user namespaces allowed here?)");
    close(to_parent[1]);
    close(to_child[0]);
    char ready;
    if (read(to_parent[0], &ready, 1) != 1) die("the namespace did not start");
    close(to_parent[0]);
    if (caller_only) {
        map_caller_only(child);
    } else if (map_ids(child)) {
        if (geteuid() == 0) die("map IDs");
        kill(child, SIGKILL);
        waitpid(child, NULL, 0);
        close(to_child[1]);
        return -1;
    }
    if (write(to_child[1], "g", 1) != 1) die("start the namespace");
    close(to_child[1]);
    return child;
}

/* Every exit, a failure too, removes the staging and scratch trees. */
static void clean_up(void) {
    if (staging[0] && access(staging, F_OK) == 0) remove_tree(staging);
    if (scratch[0] && access(scratch, F_OK) == 0) remove_tree(scratch);
}

int main(int argc, char **argv) {
    original_argv = argv;
    atexit(clean_up);
    /* Bazel runs actions with no PATH. The host tools (unsquashfs, newuidmap,
     * sudo) and what sudo runs need one: sudo without secure_path, as in the
     * GitHub Actions runner image, would not find a bare command name. */
    if (!getenv("PATH")) setenv("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", 1);
    parse(argc, argv);
    struct utsname u;
    if (uname(&u) || strcmp(u.machine, "aarch64")) {
        errno = 0;
        die("the namespace backend needs an AArch64 Linux host; this is %s", u.machine);
    }
    if (root_disk) unpack_root_disk();
    else if (!realpath(root_dir, root)) die("resolve %s", root_dir);
    char out[PATH_MAX];
    if (!realpath(outputs, out)) die("resolve %s", outputs);
    outputs = strdup(out);
    /* Next to the outputs, on the filesystem of Bazel's output base, so inputs are mostly hard links. */
    fmt(staging, sizeof staging, "%s/.krun-in.XXXXXX", outputs);
    fmt(scratch, sizeof scratch, "%s/.krun-tmp.XXXXXX", outputs);
    if (!mkdtemp(staging)) die("mkdtemp in %s", outputs);
    if (!mkdtemp(scratch)) {
        scratch[0] = 0;
        die("mkdtemp in %s", outputs);
    }
    chmod(scratch, 01777);
    if (inputs) stage_inputs();
    give_to_caller(staging);
    give_to_caller(scratch);

    pid_t child = start_guest(0);
    if (child < 0) {
        /* Refused: run again as root for this caller, else map only the caller. */
        rerun_with_sudo();
        child = start_guest(1);
    }

    int status;
    while (waitpid(child, &status, 0) < 0)
        if (errno != EINTR) die("wait");
    int code = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
    if (inputs) {
        char path[PATH_MAX], text[16] = {0};
        fmt(path, sizeof path, "%s/.krun-status", staging);
        int fd = open(path, O_RDONLY | O_CLOEXEC);
        if (fd >= 0 && read(fd, text, sizeof text - 1) > 0) {
            code = atoi(text);
        } else {
            fprintf(stderr, "krun-vm: the guest reported no exit status (namespace status %d)\n", code);
            if (code == 0) code = 1;
        }
        if (fd >= 0) close(fd);
    }
    remove_tree(staging);
    remove_tree(scratch);
    return code;
}
