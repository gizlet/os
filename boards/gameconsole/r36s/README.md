# Game Console R36S

This board builds on ROCKNIX's `rk3326-gameconsole-r36s.dts`, which describes the R36S with its original ST7703 panel. [Board tree](kernel/gizlet-r36s.dts) [Board and display reference](https://github.com/southoz/dArkOSRE-R36/tree/3f372c85aa53a26b265c3dc0c2661ec0fae5faf0/files/BOOT/dtb/r36s/R36S-V12%202023-08-18%20Panel%200)

## Writing a card

The writers ask for a language (English or 中文), the panel and the card's disk, download the matching `.img.gz` from the latest release, check it against `SHA256SUMS` and write the whole card. They list every external disk, smallest first with the first as the default answer, and flag those over 256 GB, then erase only after three confirmations: the disk's number, `y`, and the disk's name typed out. Everything on the card is lost; keep the stock card as a backup. A card that already has Gizlet OS can instead be updated (`--update`, or `GIZLET_MODE=update` for `flash.ps1`; the writers ask otherwise): only the boot code, U-Boot and partitions 1 and 2 are written, after one confirmation, and the data partition, with installed programs, saves, Wi-Fi networks and settings, is kept, as are `authorized_keys` and an unimported `setup.ini` on the boot partition. The writers refuse an update, writing nothing, unless partitions 1 and 2 match the image's and partition 3 starts where the image's does and holds its data filesystem; the partition table is written last. When the boot entry is the factory launcher, the next boot installs the updated base's launcher. Gizlet runtime 1 lives on the data partition and is not updated. They also extend the data partition to the end of the card and grow its ext4 filesystem to fill it, offline, without e2fsprogs on the computer: they write the new superblocks, group descriptors and the last group's block bitmap, a few dozen 4 KiB blocks on a 32 GB card, and the card boots with all its space at once. A card written with another tool keeps the image's 128 MiB data partition until its first boot, which extends the partition and grows the filesystem before mounting it, and so takes longer. [macOS and Linux](flash.sh) · [Windows](flash.ps1)

```sh
curl -fsSL https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.sh | bash
```

In PowerShell opened as Administrator:

```powershell
irm https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.ps1 | iex
```

`flash.sh --image FILE` writes a local `.img` or `.img.gz`, such as one from `bazel build --config=r36s //targets/sd_image/gameconsole/r36s:release`; `GIZLET_IMAGE` does the same for `flash.ps1`.

## Display variants

R36S units ship with different LCD panels. As in ROCKNIX, the device tree describes the original ST7703 panel and `boot.scr` applies `mipi-panel.dtbo` from the FAT boot partition when it is present. Each card carries the known panel overlays beside the tree; copy the one for your unit to `mipi-panel.dtbo`, or flash a card that already has it. `r36s-v30-panel4.dtbo` drives V30 boards with Panel 4 through ROCKNIX's generic DSI driver, with the init sequence converted from the stock dtb. A wrong panel boots with the backlight on and a blank screen; the stock card's `boot.ini` names the dtb that identifies the unit. `uboot.log` records which overlay was applied. [V30 Panel 4](kernel/panels/r36s-v30-panel4.dtso)

The legacy U-Boot picks a dtb name from a board-revision ADC reading before it runs `boot.scr`. Each card therefore carries its tree as `r36s.dtb` and under every name U-Boot can choose. [Card facts](card.bzl)

## TF1 storage

The 464 MiB MBR image has disk signature `36335030`. Sector sizes are 512 bytes. The boot stages, kernel load address, device tree, UART2 and OTG VBUS configuration follow the reviewed R36S boot chain. The kernel includes its ext4 driver for the data partition. [Layout](sdcard.cfg) · [Linux configuration](kernel/kernel.config) · [U-Boot commands](boot/boot.cmd)

| Start sector | Contents | Use |
| ---: | --- | --- |
| 64 | IDB, DDR initializer and miniloader | RK3326 BootROM |
| 16384 | U-Boot `LOADER` copies | U-Boot loads from `mmc 1:1` |
| 24576 | BL31 `BL3X` copies | Secure firmware |
| 32768 | Partition 1, 64 MiB FAT16 | `boot.scr`, `Image` and `r36s.dtb` |
| 163840 | Partition 2, 256 MiB allocation | `root=PARTUUID=36335030-02`, the read-only Debian base (SquashFS) |
| 688128 | Partition 3, ext4, `GIZLETDATA`: 128 MiB in the image, extended to the end of the card, filesystem included, by the card writers, or on first boot | Writable installation and user storage, with Gizlet runtime 1 |

`gizlet-data.service` identifies the mounted root device through `/proc/self/mountinfo` and sysfs. It mounts ext4 partition 3 of that same disk, with the image's filesystem UUID `17605036-fda0-4633-a95b-2ed27409a90c`. Its `gizlet/` and `home/` directories are bind-mounted at `/opt/gizlet` and `/home`. Both live on the card, and `system/` keeps the ssh host key, iwd's networks and BlueZ's pairings. `/run` and `/tmp` live in RAM, and `/var` is the image's `/var` with a RAM overlay. Boot-time setup only mounts existing filesystems. Formatting and creation of the two data directories happen while building the image. The filesystem uses `meta_bg` without `resize_inode` ([sdcard.cfg](sdcard.cfg)), so growing it only adds metadata in the new groups and never moves a block. An unmountable, read-only, mismatched or incomplete data partition results in local recovery. [Data mounts](../../../rootfs/usr/lib/gizlet/gizlet-mount-data) · [Units](../../../rootfs/usr/lib/systemd/system)

## Installing an entry

On the third partition install a trusted executable at, for example, `gizlet/bin/launcher`. Give it executable mode. Write `gizlet/boot.ini` in the following format. From the running OS the same file is `/opt/gizlet/boot.ini`.

```ini
[boot]
entry=bin/launcher
```

The file limit is 4096 bytes. The grammar accepts one `[boot]` section with one `entry=` assignment, empty lines and full-line `#` comments. Each path component accepts ASCII letters, digits, `.`, `_`, `+` and `-`. Use a relative path and real directories under `/opt/gizlet`. The Booter rejects absolute paths, `.` and `..` components, symlinks, nonregular files and files without executable mode. It executes the selected file directly with `/opt/gizlet` as the working directory. A static AArch64 Linux executable or a `#!/bin/sh` script works with this base system. Installed code runs with local root privileges. Restrict write access to the installation tree and trust the card and installed programs.

The following example installs a local smoke-test program from the recovery shell. A fresh card has no `boot.ini`: the boot session copies the factory launcher from `/usr/lib/gizlet` to `/opt/gizlet/launcher/bin/`, with its start script as `/opt/gizlet/launcher/gizlet-launcher`, and makes the script the entry. The launcher runs against Gizlet runtime 1 and draws on the LCD with OpenGL ES 2 on the GPU (Panfrost) through DRM: a home screen, settings for Wi-Fi, ssh, brightness, volume, battery, the A/B swap and the language, and a button test. Power → Quit the Launcher leaves it for the recovery shell, and deleting `boot.ini` restores the factory launcher on the next boot. `gizlet-launcher --probe` prints what it detects. [Launcher](../../../launcher)

```sh
mkdir -p /opt/gizlet/bin
printf '%s\n' '#!/bin/sh' 'echo hello-from-card' > /opt/gizlet/bin/hello
chmod 755 /opt/gizlet/bin/hello
printf '%s\n' '[boot]' 'entry=bin/hello' > /opt/gizlet/boot.ini.new
mv /opt/gizlet/boot.ini.new /opt/gizlet/boot.ini
sync
exit
```

`gizlet-session.service` runs the configured application on the kernel console, on this board UART2 at 1500000 8N1; the launcher draws on the LCD's `tty1`. No getty runs. A missing or invalid `boot.ini`, a failed exec or an application return prints a diagnostic and opens the local `recovery#` shell. Exiting that shell retries the configuration. Treat this root console and ssh as trusted development access. [Session unit](../../../rootfs/usr/lib/systemd/system/gizlet-session.service) · [Boot session](../../../rootfs/usr/lib/gizlet/boot-session)

## Network

The OTG port is a USB host with VBUS on from boot, as in the stock firmware. V30 boards carry an RTL8188EU WiFi (0bda:0179) on the same bus, powered from the PMIC's LDO9 (`vcc_wifi`); the kernel drives it with `rtl8xxxu` and built-in firmware, which also covers the RTL8188FU named by the stock kernel. The base installs iwd and OpenSSH's sshd but starts neither until they are turned on in the launcher's settings. The read-only base cannot enable units, so `gizlet-switch wifi|ssh on|off` keeps the choice on the data partition and `gizlet-switches.service` starts what is on at boot. iwd keeps its networks on the data partition. To prepare a card without the launcher, put `setup.ini` on the FAT boot partition: `wifi_ssid=` and `wifi_passphrase=` save that network and turn Wi-Fi on at every boot, `ssh=on` turns ssh on and `timezone=Asia/Shanghai` sets the time zone; without the file nothing changes. The clock comes from the RK817 RTC at boot and from network time (`systemd-timesyncd`) once Wi-Fi is connected. The time zone is UTC until one is chosen in the launcher's Date & Time settings: `/etc/localtime` links to `/run/gizlet-localtime`, which `gizlet-switch timezone` points at the chosen zone and which is linked again at every boot. [Setup](../../../rootfs/usr/lib/gizlet/gizlet-setup) sshd accepts only the public keys in `authorized_keys` on the FAT boot partition, with no passwords, and keeps its host key on the data partition. [ssh keys](../../../rootfs/usr/lib/gizlet/gizlet-ssh) · [Switches](../../../rootfs/usr/lib/gizlet/gizlet-switch)

## Bluetooth

The R36S has no Bluetooth radio of its own. A standard USB Bluetooth controller (class E0/01/01) on the OTG port works with the kernel's `btusb` driver: the kernel builds in the Bluetooth core with LE (`CONFIG_BT`, `CONFIG_BT_LE`), `CONFIG_BT_HCIBTUSB`, and `CONFIG_UHID` for BlueZ's HID-over-GATT keyboards and gamepads. Plugging one in creates `hci0`; udev then pulls in `bluetooth.target`, which starts BlueZ's `bluetoothd`. Without a controller it does not run. `bluetoothctl` and `btmgmt` drive it from the recovery shell or ssh, and its pairings live in `system/bluetooth` on the data partition. The base carries no Bluetooth firmware, so dongles that load vendor firmware from `/lib/firmware`, as most Realtek, Intel and MediaTek ones do, are not covered. An ESP32-S3 running the `gizlet/otg-device` `ble` firmware (303a:40e0, LE only) is the controller this targets. [Linux configuration](kernel/kernel.config) · [Packages](../../../rootfs/packages.json)
