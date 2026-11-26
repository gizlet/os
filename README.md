# Gizlet OS

Gizlet OS builds the Linux base system and TF1 system card for the RK3326 Game Console R36S. Its 640×480 panel is selected by a device tree overlay, as in ROCKNIX. The base is a read-only Debian trixie arm64 root with systemd, built from locked packages. It gives installed software `/opt/gizlet` and user files `/home`, and starts the executable named in `boot.ini`. A fresh card installs the Gizlet launcher as that entry. It shows the apps installed from `.gzl` packages with the `gizlet` command and starts them.

The base includes Bun at `/usr/bin/bun`. Run `bun /home/hello.js` from SSH or the recovery shell. A JavaScript file with `#!/usr/bin/bun` and executable permissions can also serve as a `.gzl` app entry or the `boot.ini` entry.

| Path | Contents |
| --- | --- |
| `boards/` | One package per device: its kernel config, device trees and panel overlays (`kernel/`), boot script and trust settings (`boot/`), card layout and test expectations |
| `rootfs/` | The Debian base every board shares: its package list and lock, and our units, scripts and configuration laid out as on the card (`etc/`, `usr/lib/gizlet/`, `usr/lib/systemd/`) |
| `session/` | The Booter, which starts the `boot.ini` entry |
| `launcher/` | The Gizlet launcher |
| `targets/` | Card images composed from the above |
| `tests/` | Card, root filesystem and QEMU tests |
| `third_party/`, `tools/bazel/` | Pinned upstream sources and the build rules |

## Build and test

Install Bazel 9.3.3 on an Apple silicon Mac. No other build tools are needed. libkrun also supports AArch64 Linux hosts with `/dev/kvm`; that path is not yet exercised here. Bazel builds a libkrun microVM and its Debian bookworm root from pinned sources, and every native stage and image test runs inside it. [Build VM](tools/bazel/krun/README.md)

```sh
bazel build --config=r36s //targets/sd_image/gameconsole/r36s:gizlet-os-r36s //targets/sd_image/gameconsole/r36s:gizlet-os-r36s-v30-panel4
bazel test --config=r36s --test_output=errors //targets/sd_image/gameconsole/r36s:image_test //targets/sd_image/gameconsole/r36s:image_test_r36s_v30_panel4 //targets/sd_image/gameconsole/r36s:rootfs_test //targets/sd_image/gameconsole/r36s:boot_test
bazel test //tools/bazel/rockchip:runner_test
```

The build writes `gizlet-os-r36s.img` under `bazel-bin/targets/sd_image/gameconsole/r36s/`, for Panel 0 (ST7703), and `gizlet-os-r36s-v30-panel4.img`, the same card with the Panel 4 (NV3051D) overlay preselected as `mipi-panel.dtbo`. Without a serial cable, `uboot.log` and `gizlet.log` on the FAT boot partition show how far the last boot got. The card test reads back the boot stages, both BL31 copies, the partition table and the device tree. The rootfs test checks the root and data filesystems, the license notices and the installed Booter, and has `e2fsck` check the data filesystem as `flash.sh` grows it on cards of 465 MiB, 32 GB and 2 TiB, and that its update keeps the data partition and refuses another card. The QEMU test starts the kernel extracted from the image on a Cortex-A35 `virt` machine. It boots a card written by another tool, whose first boot grows the data partition, then a 16 GiB card written as `flash.sh` writes it, whose first boot grows nothing. It boots that card twice, installs a test executable through the guest console, checks its output and verifies a user file across reboot. It installs, lists, runs and removes a `.gzl` app with `gizlet`, and refuses one whose files fail `CHECKSUMS`. It also exercises recovery from invalid entries, setting the time zone and hostname across a reboot, the default hostname and mDNS on virtio-net, starting bluetoothd without a controller, starting sshd on demand, an ssh login and `scp` in both directions over virtio-net, isolation from a second data disk, and a `flash.sh` update that keeps the user's files, settings and Launcher releases. The QEMU test also installs signed Launcher and CLI bundles over a local file and HTTPS, checks version selection and pruning, and verifies that a custom boot entry still works. The delivered data partition starts with empty `gizlet/` and `home/` directories.

`third_party/` pins each upstream once: Linux, Hardkernel U-Boot, Rockchip rkbin, linux-firmware and the `gizlet/rocknix` fork that serves the kernel and U-Boot patches. A board package under `boards/` builds its kernel, device trees and boot chain from those pins. A target package under `targets/` only composes the card from those labels and the rules in `tools/bazel/`. [Image rules](tools/bazel/rockchip/README.md) · [R36S board](boards/gameconsole/r36s/BUILD.bazel) · [R36S target](targets/sd_image/gameconsole/r36s/BUILD.bazel)

The board guide covers the storage contract, local recovery and application setup. Keep the original TF1 and back up an existing data partition before writing a full image. A whole-card write replaces all three partitions. Qualification of DDR, TF1, Panel 0, UART and physical controls requires a matching handheld. [R36S guide](boards/gameconsole/r36s/README.md) · [Sources and licensing](third_party/README.md)

## CI and code review

Pull requests to `main` must pass two checks. Whenever a pull request from a branch in this repository is opened, pushed to, reopened or marked ready, `.github/workflows/triage.yml` has gizlet-bot comment `/ci` and `/codex-review` for its head, and `build.yml` and `codex-review.yml` start from that comment; drafts wait until they are ready. To run one again, a user with write access comments `/ci`, `/codex-review`, or both on their own lines (👀 on the comment). Fork pull requests are refused and never built.

- `Bazel build and test (R36S)` runs the build and both tests above (`.github/workflows/build.yml`). It runs on the self-hosted ARM64 Linux runner that the `RUNNERS` organization variable maps to `linux-arm64`, with Bazel caches in `BAZEL_CACHE_DIR` on that runner; without the variable it falls back to GitHub's Ubuntu 24.04 ARM64 runner. Triage, Codex review and the check start/finish jobs run on GitHub-hosted runners.
- `Codex Code Review` reviews the diff (`.github/workflows/codex-review.yml`).

Both workflows run from `main` and use the shared [Gizlet workflows](https://github.com/gizlet/.github). Pushes to `main` build directly and refresh the Bazel caches.

## Releases

Push a `vMAJOR.MINOR.PATCH` tag on a commit in `main` to run the Release workflow. The ARM64 job builds and tests the R36S image, runtime, and Launcher. The publish job signs the Launcher bundle with the key in `secrets/release.enc.yaml` and uploads the release to the private `gizlet-assets` R2 bucket. The publish job decrypts the file with the `SOPS_AGE_KEY` organization secret.

Each version lives under `releases/<tag>/`. CI uploads `manifest.json` last. All artifact paths in the manifest are relative to the version directory. `boards.gameconsole.r36s.image` gives the compressed image and its `SHA256SUMS` path, and `flash_scripts` gives the two board-specific writers. The manifest also lists the ARM64 runtime SquashFS and the signed Launcher and CLI bundle.

Use the version directory's `flash.sh` on macOS or Linux with `jq` installed, or `flash.ps1` in an elevated PowerShell on Windows. The entry scripts ask for a language and board, then run the matching writer with that version's image and checksum. The R36S writer uses the stock card's panel information when available. For Panel 4 it applies the overlay shipped inside the same image. Blank cards still require selecting the panel.
