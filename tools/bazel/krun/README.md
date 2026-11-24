# Build VM

The native image stages and image tests run in a libkrun microVM so that one Bazel command builds and tests the card on an Apple silicon Mac. AArch64 Linux hosts with KVM are a libkrun target as well, but not yet verified with these rules. Every part of the VM is built by Bazel from pinned sources. [VM launcher](vm/main.rs) · [Rules](defs.bzl) · [Sources](repositories.bzl)

| Part | Source and build |
| --- | --- |
| `krun-vm` | libkrun 2.0 (`main` at a fixed commit) with this launcher as an added workspace member. Cargo runs offline over crates vendored by checksum. On macOS the binary is ad-hoc signed with the hypervisor entitlement. |
| Guest init | libkrun's `krun-init`, cross-compiled for `aarch64-unknown-linux-musl` with the Rust toolchain's `rust-lld` and the pinned musl target library. |
| Guest kernel | libkrunfw's prebuilt aarch64 kernel C bundle, compiled into `libkrunfw.5.dylib` by the host C compiler. |
| Root image | [`//tools/bazel/debian:rootfs`](../debian/BUILD.bazel), a reproducible SquashFS of the locked Debian bookworm arm64 build tools. A bootstrap VM whose root holds only static BusyBox unpacks the packages on its tmpfs. |

`krun_action` runs one command per VM. Inputs are linked into a private shared directory at `/in`, outputs are written to `/out`, and the command starts in a tmpfs `/tmp`. The guest writes its exit status back through `/in`, because libkrun does not report it for a disk root. `vm_test` runs a Python or Bash test the same way. `debian_cc_binary` compiles static AArch64 glibc executables with the VM's GCC.

Update the Debian lock after editing [packages.json](../debian/packages.json):

```sh
python3 -I tools/bazel/debian/update_lock.py tools/bazel/debian/packages.json
```

Set `KRUN_VM_DEBUG=1` in the host environment to see libkrun and guest kernel logs.
