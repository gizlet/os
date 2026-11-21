# R36S sources and distribution

This OS inherits the reviewed RK3326 legacy boot chain from Gizlet commit `8f51ad8cafcc576fd8a789e3b8e75fb35467b046` and the earlier GizOS board work. GizOS commit `a587fa187b752d7dd67f13d0a04ae24819e3c01f` supplies the public Rockchip Bazel rules and optional rkbin profile. That pin is the source of GizOS PR #895. The module and board manifests record the complete source choices and hashes. [Source manifest](repositories.bzl) · [Modules](../../MODULE.bazel) · [Reviewed board code](https://github.com/gizlet/gizlet/tree/8f51ad8cafcc576fd8a789e3b8e75fb35467b046) · [GizOS source](https://github.com/GizClaw/gizos/tree/a587fa187b752d7dd67f13d0a04ae24819e3c01f)

| Input | Fixed source and installed notice |
| --- | --- |
| Linux 6.12.17 | Kernel.org archive. The card carries `COPYING`, the GPL-2.0 text and Linux syscall exception in `/usr/share/licenses/linux/`. Source files retain their individual notices. |
| Board patches | The Gizlet ROCKNIX fork serves the five ordered Linux patches at `fec842b58559ee973a0100abd726370e4a032b77`. That historical object also has upstream's `20250517` tag. Its SHA-pinned files match the reviewed inputs. The fork's `next` branch has a different history. |
| U-Boot 2017.09 | Hardkernel source `2492a3e467e332e2350d987234ce6123700b3392`, with build fix and defconfig from the fork at `cefc14339a4ae18c02bb69888f27835e50664b60`, plus the local trust packer patch. General licensing information is in `/usr/share/licenses/u-boot/`. |
| Rockchip rkbin | DDR 333 MHz v2.11, miniloader v1.40 and BL31 v1.34 from `74213af1e952c4683d2e35952507133b61394862`, with the original license in `/usr/share/licenses/rockchip-rkbin`. |
| BusyBox 1.37.0 | Original BusyBox archive and the board's complete config. The card includes `/usr/share/licenses/BusyBox`. The static build also uses the builder's AArch64 glibc. |
| Booter | This repository's Booter is built with Zig's AArch64 Linux musl toolchain. The card includes musl and Zig notices, the glibc LGPL text and the inherited GizOS Apache-2.0 notice. |

The board DTS retains its SPDX identifier. The complete 4 MiB BL31 trust output must match the checked-in reference digest. [Kernel patch set in fork](https://github.com/gizlet/rocknix/tree/fec842b58559ee973a0100abd726370e4a032b77/projects/Rockchip/patches/linux/RK3326) · [U-Boot selection in fork](https://github.com/gizlet/rocknix/tree/cefc14339a4ae18c02bb69888f27835e50664b60/projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy) · [rkbin manifest](https://github.com/GizClaw/gizos/blob/a587fa187b752d7dd67f13d0a04ae24819e3c01f/tools/bazel/native/versions/rk3326_legacy_rkbin.json) · [trust SHA-256](trust.sha256)

## Corresponding source

When distributing images, provide the matching Linux, U-Boot and BusyBox source archives, their licenses, all applied board and build patches, defconfigs, DTS files and the scripts and configuration used to build and install the binaries. Include this OS revision and the referenced GizOS rules. Record which compiler and C libraries were actually used and provide the corresponding library materials and relinking support required for those static binaries. Preserve Rockchip's license with its firmware. Keep the source package, version manifest and hashes available to image recipients through the release's source delivery mechanism.
