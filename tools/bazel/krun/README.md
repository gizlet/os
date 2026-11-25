# Build VM

The native image stages and image tests run in the pinned Debian bookworm arm64 build root, so one Bazel command builds and tests the card on an Apple silicon Mac or an AArch64 Linux host with the same tools. Bazel picks how by the host:

- **macOS:** a libkrun microVM. Every part of the VM is built by Bazel from pinned sources.
- **AArch64 Linux:** Linux namespaces. The host already runs the stages' architecture, so no VM and no `/dev/kvm` are needed, and the stages see the same root, `/in`, `/out` and `/tmp`. This includes containers such as a Kubernetes runner pod.

[VM launcher](vm/main.rs) · [Namespace launcher](namespace/namespace.c) · [Rules](defs.bzl) · [Sources](repositories.bzl)

| Part | Source and build |
| --- | --- |
| `krun-vm` | libkrun 2.0 (`main` at a fixed commit) with this launcher as an added workspace member. Cargo runs offline over crates vendored by checksum. On macOS the binary is ad-hoc signed with the hypervisor entitlement. |
| Guest init | libkrun's `krun-init`, cross-compiled for `aarch64-unknown-linux-musl` with the Rust toolchain's `rust-lld` and the pinned musl target library. |
| Guest kernel | libkrunfw's prebuilt aarch64 kernel C bundle, compiled into `libkrunfw.5.dylib` by the host C compiler. |
| Root image | [`//tools/bazel/debian:rootfs`](../debian/BUILD.bazel), a reproducible SquashFS of the locked Debian bookworm arm64 build tools, including mmdebstrap, which installs the card's Debian base. A bootstrap VM whose root holds only static BusyBox unpacks the packages on its tmpfs. |

`krun_action` runs one command per VM. Inputs are linked into a private shared directory at `/in`, outputs are written to `/out`, and the command starts in a tmpfs `/tmp`. The guest writes its exit status back through `/in`, because libkrun does not report it for a disk root. `vm_test` runs a Python or Bash test the same way. `debian_cc_binary` compiles static AArch64 glibc executables with the VM's GCC.

Update the Debian lock after editing [packages.json](../debian/packages.json):

```sh
python3 -I tools/bazel/debian/update_lock.py tools/bazel/debian/packages.json
```

Set `KRUN_VM_DEBUG=1` in the host environment to see libkrun and guest kernel logs.

## Linux hosts

The namespace launcher needs, on the host:

- User namespaces, which container runtimes allow by default. A Kubernetes pod needs no privileges or devices.
- `unsquashfs` (Debian's `squashfs-tools`). Linux does not mount SquashFS in a user namespace, so the build root image is unpacked once, beside the image, and reused.
- 65536 user and group IDs, because the card's base keeps package file owners:
  - **As root**, as in many containers, nothing else is needed.
  - **As a normal user**, give the user a range in `/etc/subuid` and `/etc/subgid`, and install `uidmap` for `newuidmap`. In a container the kernel refuses that map when the user creates the namespace, so the launcher then runs itself again with passwordless `sudo -n`, as a CI runner's user has. Root then creates the namespace with the same map, and what it leaves behind belongs to the user. Without a range for the user, root maps IDs from 100000.
  - Without either, only the user is mapped, and building the base fails where packages chown files.

`/tmp` in a stage is a directory on disk beside its outputs, not memory, and `--cpus` and `--mem` do not apply.
