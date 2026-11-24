# Gizlet OS

Gizlet OS builds the Linux base system and TF1 system card for the genuine RK3326 R36S-V12 (2023-08-18) with original 640×480 Panel 0. BusyBox init mounts a read-only Linux root, gives installed software `/opt/gizlet` and user files `/home`, and starts the executable named in `boot.ini`. A fresh card opens a local recovery shell for installation.

## Build and test

Use Bazel 9.2.0 on a case-sensitive Linux filesystem. On Ubuntu 24.04 ARM64 the build and verification tools can be installed with the command below. On an amd64 builder also install `gcc-aarch64-linux-gnu` and `libc6-dev-arm64-cross`.

```sh
sudo apt-get install build-essential bc bison flex libssl-dev libelf-dev pkg-config xz-utils bzip2 patch \
    device-tree-compiler genimage dosfstools mtools squashfs-tools e2fsprogs faketime fdisk u-boot-tools python3 qemu-system-arm
bazel build --config=r36s //targets/sd_image/gameconsole/r36s:gizlet-os-r36s-panel0
bazel test --config=r36s --test_output=all //targets/sd_image/gameconsole/r36s:image_test //targets/sd_image/gameconsole/r36s:boot_test
```

The unprivileged build writes `bazel-bin/targets/sd_image/gameconsole/r36s/gizlet-os-r36s-panel0.img`. The artifact test reads back the boot stages, both BL31 copies, partition table, filesystems and Booter. The QEMU test starts the kernel extracted from the image on a Cortex-A35 `virt` machine. It boots a private writable copy twice, installs a test executable through the guest console, checks its output and verifies a user file across reboot. It also exercises recovery from invalid entries and isolation from a second data disk. The delivered data partition starts with empty `gizlet/` and `home/` directories.

The board guide covers the storage contract, local recovery and application setup. Keep the original TF1 and back up an existing data partition before writing a full image. A whole-card write replaces all three partitions. Qualification of DDR, TF1, Panel 0, UART and physical controls requires a matching handheld. [R36S guide](boards/gameconsole/r36s/rk3326/README.md) · [Sources and licensing](third_party/r36s/README.md)

## CI and code review

Pull requests to `main` must pass two checks. For every push to a pull request whose branch is in this repository, `.github/workflows/triage.yml` starts both. Fork pull requests start neither automatically and are never built.

- `Bazel build and test (R36S)` runs the build and both tests above (`.github/workflows/build.yml`). It runs on the self-hosted ARM64 Linux runner named by the `BUILD_RUNNER` organization variable, with Bazel caches in `BAZEL_CACHE_DIR` on that runner; without the variable it falls back to GitHub's Ubuntu 24.04 ARM64 runner. Only triage and Codex review run on GitHub-hosted runners. A user with write access can rerun it by commenting `/test`.
- `Codex Code Review` reviews the diff (`.github/workflows/codex-review.yml`). A user with write access can rerun it by commenting `/codex-review`.

Both workflows run from `main` and use the shared [Gizlet workflows](https://github.com/gizlet/.github). Pushes to `main` build directly and refresh the Bazel caches.
