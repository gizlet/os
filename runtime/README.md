# Gizlet runtime

A Gizlet runtime is the set of libraries that launchers and apps run against, separate from the read-only base: base updates do not change what apps see, and a runtime can be updated without writing a new card. It is not specific to one board: runtime 1 serves any arm64 Gizlet OS device.

Runtime 1 holds Debian trixie's own builds of glibc, Mesa 25.0, libdrm and their dependencies, locked in [packages.lock.json](packages.lock.json) from [packages.json](packages.json) with `"required": false`, so it carries libraries only. Mesa brings every Gallium driver Debian builds, Panfrost among them, and llvmpipe, which renders on the CPU where no GPU driver is available. llvmpipe compiles shaders with LLVM at run time, so the runtime carries libLLVM 19.

The runtime provides raylib 6.0 as `libraylib.so.600`, built for DRM with OpenGL ES 3.0 through EGL and GBM. It uses the launcher's vendored raylib-sys sources and presents frames by flipping scanout buffers. Apps can link against the runtime library with `-l:libraylib.so.600` and run through `gizlet-run`. [raylib build](build_raylib.sh)

A card carries it as `/opt/gizlet/runtime/1.sqfs` on the data partition, a read-only SquashFS image. `gizlet-mount-data` mounts every `runtime/*.sqfs` beside its file, so runtime 1 appears at `/opt/gizlet/runtime/1`:

| Path | Contents |
| --- | --- |
| `lib/` | glibc with its loader `ld-linux-aarch64.so.1`, libglvnd's `libEGL.so.1` and `libGLESv2.so.2`, Mesa's `libEGL_mesa.so.0`, `libgbm.so.1` and `libgallium`, libdrm, LLVM, raylib |
| `lib/dri/`, `lib/gbm/` | Mesa's drivers and its GBM backend |
| `share/glvnd/egl_vendor.d/` | The EGL vendor file naming Mesa |
| `share/doc/PACKAGE/copyright` | Each package's copyright and license |
| `bin/gizlet-run` | Runs a program with the runtime's loader, libraries and drivers |
| `bin/gizlet-gl-info` | Prints the OpenGL ES renderer and checks an off-screen render |

Fresh cards include the runtime built with the image. The card writer's `--update` mode preserves the existing runtime on the data partition. Existing cards use the ES 3.0 raylib build once their `1.sqfs` has also been updated.

```sh
/opt/gizlet/runtime/1/bin/gizlet-run /opt/gizlet/runtime/1/bin/gizlet-gl-info
```

The probe requests an OpenGL ES 3.0 context. The R36S should report `Mali-G31 (Panfrost)` on `/dev/dri/renderD128`. QEMU, without a render node, reports llvmpipe on EGL's surfaceless platform. [Build](build_runtime.sh) · [Probe](gl-info.c)

```sh
bazel build --config=r36s //runtime:gizlet-runtime-1
```
