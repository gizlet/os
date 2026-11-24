# R36S-V12 Panel 0

This card targets the genuine standard RK3326 R36S-V12 marked `2023-08-18`, with the original ST7703 Panel 0. Verify the original card's display DTB as well as the PCB marking. [Board and display reference](https://github.com/southoz/dArkOSRE-R36/tree/3f372c85aa53a26b265c3dc0c2661ec0fae5faf0/files/BOOT/dtb/r36s/R36S-V12%202023-08-18%20Panel%200)

## Display variants

R36S units ship with different LCD panels. `gizlet-r36s-panel0.dts` drives the original ST7703 Panel 0. `gizlet-r36s-v30-panel4.dts` is for V30 boards with Panel 4, whose controller ROCKNIX's generic DSI driver initializes from a description converted from the stock dtb. A card for the wrong panel boots with the backlight on and a blank screen. To identify a unit, check which dtb its stock card loads in `boot.ini`. [Panel 0](gizlet-r36s-panel0.dts) · [V30 Panel 4](gizlet-r36s-v30-panel4.dts)

The legacy U-Boot picks a dtb name from a board-revision ADC reading before it runs `boot.scr`. Each card therefore carries its tree as `r36s.dtb` and under every name U-Boot can choose. [Card facts](card.bzl)

## TF1 storage

The 160 MiB MBR image has disk signature `36335030`. Sector sizes are 512 bytes. The boot stages, kernel load address, Panel 0 device tree, UART2 and OTG VBUS configuration follow the reviewed R36S boot chain. The kernel includes its ext4 driver for the data partition. [Layout](sdcard.cfg) · [Linux configuration](kernel.config) · [U-Boot commands](boot.cmd)

| Start sector | Contents | Use |
| ---: | --- | --- |
| 64 | IDB, DDR initializer and miniloader | RK3326 BootROM |
| 16384 | U-Boot `LOADER` copies | U-Boot loads from `mmc 1:1` |
| 24576 | BL31 `BL3X` copies | Secure firmware |
| 32768 | Partition 1, 64 MiB FAT16 | `boot.scr`, `Image` and `r36s.dtb` |
| 163840 | Partition 2, 16 MiB allocation | `root=PARTUUID=36335030-02`, read-only SquashFS |
| 196608 | Partition 3, 64 MiB ext4, `GIZLETDATA` | Writable installation and user storage |

Init identifies the mounted root device through `/proc/self/mountinfo` and sysfs. It mounts ext4 partition 3 of that same disk, with the image's filesystem UUID `17605036-fda0-4633-a95b-2ed27409a90c`. Its `gizlet/` and `home/` directories are bind-mounted at `/opt/gizlet` and `/home`. Both live on the card. `/run` and `/tmp` live in RAM. Boot-time setup only mounts existing filesystems. Formatting and creation of the two data directories happen while building the image. An unmountable, read-only, mismatched or incomplete data partition results in local recovery. [Early mounts](rcS)

## Installing an entry

On the third partition install a trusted executable at, for example, `gizlet/bin/launcher`. Give it executable mode. Write `gizlet/boot.ini` in the following format. From the running OS the same file is `/opt/gizlet/boot.ini`.

```ini
[boot]
entry=bin/launcher
```

The file limit is 4096 bytes. The grammar accepts one `[boot]` section with one `entry=` assignment, empty lines and full-line `#` comments. Each path component accepts ASCII letters, digits, `.`, `_`, `+` and `-`. Use a relative path and real directories under `/opt/gizlet`. The Booter rejects absolute paths, `.` and `..` components, symlinks, nonregular files and files without executable mode. It executes the selected file directly with `/opt/gizlet` as the working directory. A static AArch64 Linux executable or a `#!/bin/sh` script works with this base system. Installed code runs with local root privileges. Restrict write access to the installation tree and trust the card and installed programs.

The following example installs a local smoke-test program from the recovery shell. The delivered card is ready for an independently supplied launcher.

```sh
mkdir -p /opt/gizlet/bin
printf '%s\n' '#!/bin/sh' 'echo hello-from-card' > /opt/gizlet/bin/hello
chmod 755 /opt/gizlet/bin/hello
printf '%s\n' '[boot]' 'entry=bin/hello' > /opt/gizlet/boot.ini.new
mv /opt/gizlet/boot.ini.new /opt/gizlet/boot.ini
sync
exit
```

The active kernel console owns the configured application session. On this board it is UART2, 1500000 8N1. The LCD's `tty1` offers a local shell on Enter. A missing or invalid `boot.ini`, a failed exec or an application return prints a diagnostic and opens the local `recovery#` shell. Exiting that shell retries the configuration. The base starts no network service. Treat these root consoles as trusted physical development access. [BusyBox init table](inittab) · [Boot session](../../../../boot/boot-session)
