# Game Console R36S

This board builds on ROCKNIX's `rk3326-gameconsole-r36s.dts` and uses the V30 NV3051D (Panel 4) display configuration. [Board tree](kernel/gizlet-r36s.dts)

## Writing a card

Each release publishes one R36S image, `gizlet-os-r36s.img.gz`. The writers ask for a language (English or 中文) and the target disk, then download the image and verify it against `SHA256SUMS` before writing. The image contains the display configuration and works on a blank card. [Display support](#display-support) · [macOS and Linux writer](flash.sh) · [Windows writer](flash.ps1)

The writers list external disks from smallest to largest, offer the smallest as the default, and flag disks over 256 GB. A full write erases all data after you select the disk, answer `y`, and type its name. Keep the stock card as a backup.

A card with Gizlet OS can be updated using `--update` or `GIZLET_MODE=update` on Windows. After confirmation, the writer replaces the boot code, U-Boot and partitions 1 and 2 while keeping partition 3 with installed programs, every profile, Wi-Fi networks and settings. It also keeps `authorized_keys` and an unimported `setup.ini` on the boot partition. The writer checks that partitions 1 and 2 match the image and that partition 3 starts at the expected sector and holds the data filesystem before writing. It writes the partition table last. On the next boot, the system selects the newest compatible Launcher and CLI on the data partition, or uses the factory pair. Gizlet runtime 1 stays on the data partition.

The update mode requires a card with the local profiles layout and a V30 NV3051D boot configuration. The writers reject updates of cards configured for other displays before writing. Cards made before local profiles require a full image write. The writers can extend the data partition and grow its ext4 filesystem to fill the card offline. A card written with another tool starts with the image's 128 MiB data partition and grows it on first boot.

```sh
curl -fsSL https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.sh | bash
```

In PowerShell opened as Administrator:

```powershell
irm https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.ps1 | iex
```

## Local images and display validation

Build `:gizlet-os-r36s` to produce `gizlet-os-r36s.img`. Run this board's `flash.sh --image FILE` to write a local `.img` or `.img.gz`, or set `GIZLET_IMAGE` for `flash.ps1`. To validate the display, write the standard image to a blank TF1 card and cold boot to the Launcher.

## Display support

The current image supports the V30 NV3051D (Panel 4) configuration. The R36S tree includes the [panel profile](kernel/panels/r36s-v30-panel4.dtsi), which supplies timings and initialization commands to the built-in ROCKNIX generic DSI driver.

The driver reads DCS `04`, `DA`, `DB`, and `DC` before initialization and reports the results in `gizlet.log`. These values provide diagnostics. Boot currently uses the single built-in profile. Additional panels need their own profiles and a verified way to select them at boot.

The legacy U-Boot picks a dtb name from a board-revision ADC reading before it runs `boot.scr`. Each card therefore carries its tree as `r36s.dtb` and under every name U-Boot can choose. [Card facts](card.bzl)

## TF1 storage

The 464 MiB MBR image has disk signature `36335030`. Sector sizes are 512 bytes. The boot stages, kernel load address, device tree, UART2 and OTG VBUS configuration follow the reviewed R36S boot chain. The kernel includes its ext4 driver for the data partition. The CPU scales from 600 MHz at 0.95 V to 1.296 GHz with schedutil and the GPU from 200 to 560 MHz with devfreq, along the steps of the upstream `px30.dtsi` that ROCKNIX drops, and idle cores and the cluster power down through PSCI on a tickless kernel. The R36S image limits schedutil to one clock change every 10 ms, as each change sets `vdd_arm` over the PMIC's I2C bus. [Rate limit](config/gizlet-cpufreq.conf) [Layout](sdcard.cfg) · [Linux configuration](kernel/kernel.config) · [U-Boot commands](boot/boot.cmd)

| Start sector | Contents | Use |
| ---: | --- | --- |
| 64 | IDB, DDR initializer and miniloader | RK3326 BootROM |
| 16384 | U-Boot `LOADER` copies | U-Boot loads from `mmc 1:1` |
| 24576 | BL31 `BL3X` copies | Secure firmware |
| 32768 | Partition 1, 64 MiB FAT16 | `boot.scr`, `Image` and `r36s.dtb` |
| 163840 | Partition 2, 256 MiB allocation | `root=PARTUUID=36335030-02`, the read-only Debian base (SquashFS) |
| 688128 | Partition 3, ext4, `GIZLETDATA`: 128 MiB in the image, extended to the end of the card, filesystem included, by the card writers, or on first boot | Writable installation and user storage, with Gizlet runtime 1 |

`gizlet-data.service` identifies the mounted root device through `/proc/self/mountinfo` and sysfs. It mounts ext4 partition 3 of that same disk, with the image's filesystem UUID `cef58f4f-a7a3-40ac-9cb3-bd8d89aab65f`, at `/data`. Its `device/` directory holds what every local profile shares: installed packages, runtimes, Launcher releases, and in `settings/` the ssh host key, iwd's networks, BlueZ's pairings and the other device settings. `profiles/` holds one directory per [profile](../../../launcher/README.md#profiles). The profile named in `device/boot-profile`, the oldest one without it, or a new one on a fresh card, is bind-mounted at `/run/gizlet/profile`, and its `home/` at `/home`. `/run` and `/tmp` live in RAM, and `/var` is the image's `/var` with a RAM overlay. Boot-time setup only mounts existing filesystems. Formatting and creation of `device/` and `profiles/` happen while building the image; the first boot creates the first profile. The filesystem uses `meta_bg` without `resize_inode` ([sdcard.cfg](sdcard.cfg)), so growing it only adds metadata in the new groups and never moves a block. An unmountable, read-only, mismatched or incomplete data partition results in local recovery. `gizlet.log` lists the seconds since boot at each step of the data mount (`gizlet-mount-data steps:`). [Data mounts](../../../rootfs/usr/lib/gizlet/gizlet-mount-data) · [Units](../../../rootfs/usr/lib/systemd/system)

## TF2 card

The second slot holds the user's own card, such as one with their games. When a filesystem appears on an MMC disk, udev starts `gizlet-tf2@<device>.service` ([rule](../../../rootfs/usr/lib/udev/rules.d/90-gizlet-tf2.rules)). `gizlet-tf2` refuses every partition of the disk that holds the root filesystem, so TF1 is never mounted there. Of the other card's partitions, or the whole card when it has none, it mounts the largest FAT, exFAT or ext2/3/4 filesystem at `/media/tf2`, with `nosuid,nodev,noexec,noatime`, FAT and exFAT files owned by root and readable by all, and UTF-8 names. Removing the card stops the unit, which unmounts it. The card stays the user's: nothing formats, checks or repairs it, and the system keeps nothing on it. Apps find it at `/media/tf2`, or as the mount on an MMC disk other than the root's. [Script](../../../rootfs/usr/lib/gizlet/gizlet-tf2)

## Installing apps

Apps come as `.gzl` packages. Copy one to the handheld and install it over ssh; it appears on the launcher's home screen, where A starts it. Packages, where apps live and how they run are described in the [launcher](../../../launcher/README.md#apps).

```sh
scp io.gizlet.overeaten-0.1.0-aarch64-unknown-linux-gnu-lite.gzl root@DEVICE:/tmp/
ssh root@DEVICE gizlet install /tmp/io.gizlet.overeaten-0.1.0-aarch64-unknown-linux-gnu-lite.gzl
ssh root@DEVICE gizlet start io.gizlet.overeaten
```

Hold FN for a second, or Select and Start for two, to end any app and return to the launcher.

## Installing an entry

On the third partition install a trusted executable at, for example, `device/bin/launcher`. Give it executable mode. Write `device/boot.ini` in the following format. From the running OS the same file is `/data/device/boot.ini`. The entry belongs to the device, so every profile boots it.

```ini
[boot]
entry=bin/launcher
```

The file limit is 4096 bytes. The grammar accepts one `[boot]` section with one `entry=` assignment, empty lines and full-line `#` comments. Each path component accepts ASCII letters, digits, `.`, `_`, `+` and `-`. Use a relative path and real directories under `/data/device`. The Booter rejects absolute paths, `.` and `..` components, symlinks, nonregular files and files without executable mode. It executes the selected file directly with `/data/device` as the working directory. A static AArch64 Linux executable or a `#!/bin/sh` script works with this base system. Installed code runs with local root privileges. Restrict write access to the installation tree and trust the card and installed programs.

The following example installs a local smoke-test program from the recovery shell. A fresh card has no `boot.ini`, so it starts the factory Launcher from the read-only rootfs. Installing a signed [Launcher release](../../../launcher/README.md#updating-the-launcher-and-cli) makes it available at the next boot. `/usr/bin/gizlet` uses the CLI from the release selected at boot, including when `boot.ini` selects a custom entry. The launcher runs against Gizlet runtime 1 and draws on the LCD with OpenGL ES 3.0 on the GPU (Panfrost) through DRM: a home screen, settings for Wi-Fi, ssh, brightness, volume, battery, the A/B swap and the language, and a button test. Power → Quit the Launcher opens the recovery shell. Deleting `boot.ini` restores the managed Launcher as the boot entry. `/usr/lib/gizlet/launcher-release start --probe` prints what the selected Launcher detects. [Launcher](../../../launcher)

```sh
mkdir -p /data/device/bin
printf '%s\n' '#!/bin/sh' 'echo hello-from-card' > /data/device/bin/hello
chmod 755 /data/device/bin/hello
printf '%s\n' '[boot]' 'entry=bin/hello' > /data/device/boot.ini.new
mv /data/device/boot.ini.new /data/device/boot.ini
sync
exit
```

`gizlet-session.service` runs the configured application on the kernel console, on this board UART2 at 1500000 8N1; the launcher draws on the LCD's `tty1`. No getty runs. An invalid `boot.ini`, a failed exec or an application return prints a diagnostic and opens the local `recovery#` shell. Exiting that shell retries the configuration. Treat this root console and ssh as trusted development access. [Session unit](../../../services/session/gizlet-session.service) · [Boot session](../../../services/session/boot-session)

## Network

The OTG port is a USB host with VBUS on from boot, as in the stock firmware. V30 boards carry an RTL8188EU WiFi (0bda:0179) on the same bus, powered from the PMIC's LDO9 (`vcc_wifi`); the kernel drives it with `rtl8xxxu` and built-in firmware, which also covers the RTL8188FU named by the stock kernel. The base installs iwd and OpenSSH's sshd but starts neither until they are turned on in the launcher's settings. The read-only base cannot enable units, so `gizlet-switch wifi|ssh on|off` keeps the choice on the data partition and `gizlet-switches.service` starts what is on at boot. iwd keeps its networks on the data partition, and starting it starts `systemd-resolved`, which no longer starts at boot. To prepare a card without the launcher, put `setup.ini` on the FAT boot partition: `wifi_ssid=` and `wifi_passphrase=` save that network and turn Wi-Fi on at every boot, `ssh=on` turns ssh on and `timezone=Asia/Shanghai` sets the time zone; without the file nothing changes. The clock comes from the RK817 RTC at boot and from network time (`systemd-timesyncd`) once Wi-Fi is connected, asking `time.apple.com` and then `ntp.aliyun.com`. The time zone is UTC until one is chosen in the launcher's Date & Time settings: `/etc/localtime` links to `/run/gizlet-localtime`, which `gizlet-switch timezone` points at the chosen zone and which is linked again at every boot. The device is named `gizlet-` and the last four hex digits of its Wi-Fi adapter's MAC address, such as `gizlet-a1b2`, or of another network adapter's without one, and `gizlet` without any. `gizlet-hostname.service` sets the name at boot and again when udev adds a network adapter, as the USB Wi-Fi may appear later. Another name can be chosen in the launcher's System settings or over ssh with `gizlet-switch hostname NAME` (one DNS label, lowercased; an empty name goes back to the default), and is kept on the data partition. While Wi-Fi is on, `systemd-resolved` answers for `NAME.local` by multicast DNS, so `ssh root@gizlet-a1b2.local` reaches the device without knowing its address. [Setup](../../../rootfs/usr/lib/gizlet/gizlet-setup) sshd accepts only the public keys in `authorized_keys` on the FAT boot partition, with no passwords, and keeps its host key on the data partition. [ssh keys](../../../rootfs/usr/lib/gizlet/gizlet-ssh) · [Switches](../../../rootfs/usr/lib/gizlet/gizlet-switch)

## Bluetooth

The R36S has no Bluetooth radio of its own. A standard USB Bluetooth controller (class E0/01/01) on the OTG port works with the kernel's `btusb` driver: the kernel builds in the Bluetooth core with LE (`CONFIG_BT`, `CONFIG_BT_LE`), `CONFIG_BT_HCIBTUSB`, and `CONFIG_UHID` for BlueZ's HID-over-GATT keyboards and gamepads. Plugging one in creates `hci0`; udev then pulls in `bluetooth.target`, which starts BlueZ's `bluetoothd`. Without a controller it does not run. `bluetoothctl` and `btmgmt` drive it from the recovery shell or ssh, and its pairings live in `device/settings/bluetooth` on the data partition. The base carries no Bluetooth firmware, so dongles that load vendor firmware from `/lib/firmware`, as most Realtek, Intel and MediaTek ones do, are not covered. An ESP32-S3 running the `gizlet/otg-device` `ble` firmware (303a:40e0, LE only) is the controller this targets. [Linux configuration](kernel/kernel.config) · [Packages](../../../rootfs/packages.json)

## Audio

The R36S image enables `gizlet-audio-jack.service` with its RK817 mixer settings. On startup and on each jack change, the service sets `Playback Mux` to `HP` for headphones or `SPK` for the speaker. [Board settings](config/gizlet-audio-jack.conf) · [Service](../../../services/audio-jack/gizlet-audio-jack.service)

## Sleep

`gizlet-power` suspends the R36S to RAM (`deep`, through PSCI) after the launcher's Sleep on Battery or Sleep While Charging time without input (`sleep_after` and `sleep_after_charging` in `power.ini`, 5 minutes and never by default; plugging in or unplugging counts as use), dimming the screen 15 s before, on a press of the power key, or from the power menu's Sleep. The power key wakes it. It arms `/sys/power/wakeup_count` first, so a wakeup event while it gets ready cancels the sleep and turns the screen back on. Both card slots are powered from the RK817's LDO6 (`vcc_sd`), and the card-detect line of an empty TF2 slot is pulled up to that rail. When TF1's controller suspended, it switched the rail off. The line fell, the MMC core took that for an inserted card and held a wakeup event, and every sleep was cancelled. The board tree therefore keeps `vcc_sd` on, as the PMIC already does in sleep. Wi-Fi does not stay connected across sleep: `rtl8xxxu` has no suspend support, so the USB core unbinds it on suspend and binds it again on resume. iwd then reconnects to the network within about a second, and ssh sessions open across a sleep are dropped. `vcc_wifi` stays on. On a V30 board, the battery gauge's counter falls by about 20 mA while asleep, against several hundred milliamps awake. [Power](../../../services/power/power.c) · [Board tree](kernel/gizlet-r36s.dts)

The RK817 reports its charger offline and the battery `Not charging` while it charges. The R36S power service configuration treats `Not charging` as external power. The launcher uses the battery current to distinguish charging from plugged in. [Board settings](config/gizlet-power.conf)
