# Rockchip image rules

These four rules build the R36S card from labels supplied by the board target. They are derived from GizOS commit `0ac3bfdc2bafa78bfc89965931b5ce22faeff795` under Apache-2.0 and are maintained in this repository. Board targets load them from `//tools/bazel/rockchip:defs.bzl`. [Declarations](defs.bzl) · [Native stages](runner.py) · [R36S target](../../../targets/sd_image/gameconsole/r36s/BUILD.bazel)

| Rule | Inputs | Outputs |
| --- | --- | --- |
| `rockchip_linux_kernel` | Single-root Linux archive, ordered `patches`, `config`, Kbuild `dtb_target` and optional `dts_files` mapped under `arch/arm64/boot/dts/` | `image`, `dtb` |
| `rockchip_legacy_uboot` | Single-root U-Boot archive, ordered `patches`, `<board>_defconfig`, DDR init, miniloader, BL31 ELF, `trust_ini`, optional `trust_checksum`, boot command, script title, `mkimage_soc` and `load_address` | `idbloader`, `uboot`, `trust`, `script` |
| `busybox_squashfs` | Single-root BusyBox archive, `config` over `allnoconfig` with `CONFIG_STATIC=y`, optional patches, directories, data files (0644), executables (0755) and files from the unpacked source | `out`, a root-owned SquashFS with fixed times |
| `rockchip_sd_image` | genimage `layout`, `layout_image` entry name and a label-to-input-path map | `out`, the raw card image |

Every rule takes `source_date_epoch`. Patches apply with GNU `patch -p1 --fuzz=0` in list order. Kernel and BusyBox builds fail when a requested `CONFIG_…=…` setting does not survive configuration. Destination paths must be unique and stay inside their staging tree.

The actions run `/usr/bin/python3` outside the sandbox on Linux and use the host's build tools listed in the [top-level README](../../../README.md#build-and-test). Because those tools are not Bazel inputs, the workspace status command must print a fresh `STABLE_ROCKCHIP_BUILD_ID` UUID per invocation. The `--config=r36s` profile uses [build_status.sh](../../r36s/build_status.sh) for this.

Run the runner's unit test in the default host configuration:

```sh
bazel test //tools/bazel/rockchip:runner_test
```
