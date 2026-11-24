# Gizlet runtime

A Gizlet runtime is the set of libraries that launchers and apps run against, separate from the read-only base: base updates do not change what apps see, and a runtime can be updated without writing a new card. It is not specific to one board: runtime 1 serves any arm64 Gizlet OS device.

Runtime 1 holds Debian trixie's own builds of glibc, Mesa 25.0, libdrm and their dependencies, locked in [packages.lock.json](packages.lock.json) from [packages.json](packages.json) with `"required": false`, so it carries libraries only. Mesa brings every Gallium driver Debian builds, Panfrost among them, and llvmpipe, which renders on the CPU where no GPU driver is available. llvmpipe compiles shaders with LLVM at run time, so the runtime carries libLLVM 19.

The runtime adds raylib 6.0 as `libraylib.so.600`, built here for DRM with OpenGL ES 2 through EGL and GBM, with `SUPPORT_DRM_CACHE` so frames are presented by page flips on two scanout buffers. Its sources are the launcher's vendored raylib-sys crate, so the launcher's bindings match it. [raylib build](build_raylib.sh)

A card carries it as `/opt/gizlet/runtime/1.sqfs` on the data partition, a read-only SquashFS image. `gizlet-mount-data` mounts every `runtime/*.sqfs` beside its file, so runtime 1 appears at `/opt/gizlet/runtime/1`:

| Path | Contents |
| --- | --- |
| `lib/` | glibc with its loader `ld-linux-aarch64.so.1`, libglvnd's `libEGL.so.1` and `libGLESv2.so.2`, Mesa's `libEGL_mesa.so.0`, `libgbm.so.1` and `libgallium`, libdrm, LLVM, raylib |
| `lib/dri/`, `lib/gbm/` | Mesa's drivers and its GBM backend |
| `share/glvnd/egl_vendor.d/` | The EGL vendor file naming Mesa |
| `share/doc/PACKAGE/copyright` | Each package's copyright and license |
| `bin/gizlet-run` | Runs a program with the runtime's loader, libraries and drivers |
| `bin/gizlet-gl-info` | Prints the OpenGL ES renderer and checks an off-screen render |

```sh
/opt/gizlet/runtime/1/bin/gizlet-run /opt/gizlet/runtime/1/bin/gizlet-gl-info
```

The R36S should report `Mali-G31 (Panfrost)` on `/dev/dri/renderD128`; QEMU, without a render node, reports llvmpipe on EGL's surfaceless platform. [Build](build_runtime.sh) · [Probe](gl-info.c)

```sh
bazel build --config=r36s //runtime:gizlet-runtime-1
```
