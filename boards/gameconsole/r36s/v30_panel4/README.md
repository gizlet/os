# R36S V30 / Panel 4 RAM boot trial

The trial starts a Gizlet OS BusyBox system from TF1's existing FAT BOOT partition. The stock-kernel option reuses the card's working display path. The second option builds an ARM64 Linux 6.12.17 Image and matching device tree from source. Both start the same `/init` entirely in RAM, run the Gizlet Booter and open a recovery shell. The RAM trial only mounts devtmpfs, procfs and sysfs.

## Stock kernel

The matched ArkOS4Clone profile is `consoles/v30 panel4` from commit `3c78add9192bea6857a85ae23a5c21a086281873`. The card booted by this profile uses these filenames and SHA-256 digests. [Linux DTB](https://github.com/lcdyk0517/arkos4clone/tree/3c78add9192bea6857a85ae23a5c21a086281873/consoles/v30%20panel4) · [Linux 4.4.189 Image](https://github.com/lcdyk0517/arkos4clone/tree/3c78add9192bea6857a85ae23a5c21a086281873/consoles/kernel/arkos4clone_fix)

| File already on BOOT | SHA-256 |
| --- | --- |
| `Image` | `4c55faf6faf60f6b84de529f126831f4a51eb7f5bf65856693ed25a5d5c620cd` |
| `rk3326-r36s-v30-linux.dtb` | `5e03b679d6f5c56f8ab88783ad61f25391ad3e7e86085a602d0db71d4e501210` |

Use the Linux Bazel environment from the repository README. These commands stage files in a new **local directory** under the current checkout.

```sh
pkg=//targets/sd_image/gameconsole/r36s/v30_panel4
board=boards/gameconsole/r36s/v30_panel4
files=bazel-bin/targets/sd_image/gameconsole/r36s/v30_panel4/gizlet-test
bazel build --config=r36s-v30 "$pkg:stock_trial"
mkdir -p out/stock/BOOT/gizlet-test
cp "$files/uInitrd" out/stock/BOOT/gizlet-test/
cp "$board/boot.ini" out/stock/BOOT/boot.ini
(cd out/stock/BOOT && sha256sum boot.ini gizlet-test/uInitrd)
```

On a matching TF1 with a backup, copy `gizlet-test/uInitrd` onto BOOT first and select the trial with `boot.ini` last. The stock script loads `Image`, `gizlet-test/uInitrd` and `rk3326-r36s-v30-linux.dtb` from `mmc 1:1` into `0x02000000`, `0x01100000` and `0x01f00000`. The three successful loads lead to `booti`. The kernel command line selects `/init`, `tty0`, Rockchip `ttyFIQ0` and the card's original CPU, GPU and DDR frequencies. The card's original `boot.ini` selects the installed ArkOS system again.

## Source-built kernel

```sh
bazel build --config=r36s-v30 "$pkg:mainline_trial"
bazel test --config=r36s-v30 --test_output=all "$pkg:boot_test"
mkdir -p out/mainline/BOOT/gizlet-test
cp "$files/Image" "$files/board.dtb" "$files/uInitrd" out/mainline/BOOT/gizlet-test/
cp "$board/boot-mainline.ini" out/mainline/BOOT/boot.ini
(cd out/mainline/BOOT && sha256sum boot.ini gizlet-test/*)
```

This option loads the three staged `gizlet-test/` files. The V30 profile extends the four-lane NV3051D MIPI driver with the card's 151 DCS register writes. Exit sleep (`0x11`, 200 ms) and display on (`0x29`, 20 ms) complete the 153-packet sequence. The Linux DTB on the card supplies the 58 MHz, 640×480 mode with horizontal porches 300/200/300 and vertical porches 17/5/13 (front/sync/back). The board supplies the panel from RK817 LDO8 at 2.8 V, asserts reset on GPIO3_C0, and drives the backlight with PWM1 at 62,745 ns with default brightness 51/255. The vendor DSI mode flags `0xa03` select video burst, disabled HS EoT and low-power commands. [Board DTS](gizlet-r36s-v30-panel4.dts) · [Panel driver patch](nv3051d-v30.patch)

## Observing a cold boot

The boot script's messages follow the existing U-Boot console. The Linux framebuffer console can display kernel logs once the VOP, DSI, panel and backlight register. The source-built kernel has these drivers built in. Its UART2-M1 uses `ttyS2` at 115200 8N1, with the early UART at `0xff160000`. The stock DTB uses the FIQ driver on this UART. Boot stages before framebuffer registration can be observed through the serial console when available.

Watch for `RAM /init reached`, the Linux version, `Starting Gizlet Booter`, then `GIZLET PANEL TEST READY`. `/init` writes progress to both the active console and `tty1`. It reports the backlight level and framebuffer name and replays `/run/boot.log` when the framebuffer appears. A missing framebuffer after 15 seconds or a failed Booter entry produces a diagnostic. The serial session opens `gizlet-trial#`, and Enter on `tty1` starts a local BusyBox shell when keyboard input is available. `dmesg`, `cat /run/boot.log` and `cat /proc/mounts` work from recovery.

The QEMU test boots the built Image and ramdisk on ARM64 `virt` with PL011 and a 640×480 virtio GPU. It checks the normal entry, a deliberately missing entry, interactive recovery, mounted filesystems and the text on `tty1`. A cold boot on the matching handheld tests the existing boot firmware, panel, PWM backlight, physical UART and input.
