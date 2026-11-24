# Rockchip image rules

These rules build and check the R36S card from labels supplied by the board target. They are derived from GizOS commit `0ac3bfdc2bafa78bfc89965931b5ce22faeff795` under Apache-2.0 and are maintained in this repository. [License](LICENSE) applies to these build files and the test toolchain in `tools/bazel/platforms`; nothing from them is installed on the card. Board targets load them from `//tools/bazel/rockchip:defs.bzl`. [Declarations](defs.bzl) · [Native stages](runner.py) · [R36S target](../../../targets/sd_image/gameconsole/r36s/BUILD.bazel)

| Rule | Inputs | Outputs |
| --- | --- | --- |
| `rockchip_linux_kernel` | Single-root Linux archive, ordered `patches`, `config`, Kbuild `dtb_target` and optional `dts_files` mapped under `arch/arm64/boot/dts/` | `image`, `dtb` |
| `rockchip_legacy_uboot` | Single-root U-Boot archive, ordered `patches`, `<board>_defconfig`, DDR init, miniloader, BL31 ELF, `trust_ini`, optional `trust_checksum`, boot command, script title, `mkimage_soc` and `load_address` | `idbloader`, `uboot`, `trust`, `script` |
| `rockchip_sd_image` | genimage `layout`, `layout_image` entry name and a label-to-input-path map | `out`, the raw card image |

Every rule takes `source_date_epoch`. Patches apply with GNU `patch -p1` in list order, with zero fuzz unless the target sets `patch_fuzz`. Kernel builds fail when a requested `CONFIG_…=…` setting does not survive configuration. Destination paths must be unique and stay inside their staging tree.

Each stage runs [runner.py](runner.py) in the libkrun build VM on its read-only Debian root. Inputs are staged into the guest, and the source trees and build directories live on the guest's case-sensitive tmpfs. All tools are Bazel inputs, so the stages cache and rebuild like ordinary actions. [Build VM](../krun/README.md)

`rockchip_card_test` checks a finished card in the same VM: MBR, card size, IDB digest, both U-Boot and BL31 copies, the boot script and the board's device tree expectations. Boards describe their card in a `.bzl` dict. [Card test](../../../tests/card_test.py) · [R36S facts](../../../boards/gameconsole/r36s/card.bzl)

```sh
bazel test //tools/bazel/rockchip:runner_test
```
