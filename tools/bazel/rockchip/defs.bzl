"""Build rules for ARM64 Rockchip images. Each stage runs in the libkrun build VM. See README.md.

Derived from GizOS tools/bazel/native/rockchip at 0ac3bfdc2bafa78bfc89965931b5ce22faeff795 (Apache-2.0, see LICENSE).
"""

load("//tools/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action", "vm_test")

_NATIVE = KRUN_ATTRS | {
    "source_date_epoch": attr.int(mandatory = True, doc = "Fixed Unix timestamp for the image tools."),
    "_runner": attr.label(default = Label(":runner.py"), allow_single_file = True),
    "_rootfs": attr.label(default = Label("//tools/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
}
_SOURCE = {
    "archive": attr.label(mandatory = True, allow_single_file = True, doc = "Pinned source tarball with one leading directory."),
    "patches": attr.label_list(allow_files = True, doc = "Ordered patches applied with GNU patch -p1."),
    "patch_fuzz": attr.int(default = 0, doc = "GNU patch --fuzz. Zero requires exact context; 2 matches plain `patch`."),
}
_KBUILD = {
    "build_host": attr.string(default = "builder"),
    "build_user": attr.string(default = "builder"),
}

# Guest resources per stage: (vCPUs, MiB). The tmpfs holding the source tree
# and build directory uses most of the guest memory.
_RESOURCES = {
    "Kernel": (8, 8192),
    "LegacyUBoot": (4, 4096),
    "BusyBoxSquashfs": (4, 2048),
    "SDImage": (2, 2048),
}

def _stage(ctx, stage, inputs, outputs, params):
    """Runs one runner stage in the build VM.

    Inputs appear under /in at their execroot paths, which is the runner's
    working directory. Outputs are written under /out at their package paths.
    """
    if ctx.attr.source_date_epoch < 0:
        fail("source_date_epoch must be nonnegative")
    out_root = ctx.bin_dir.path + "/" + ctx.label.package + "/"
    params["outputs"] = ["/out/" + output.path.removeprefix(out_root) for output in outputs]
    cpus, mem = _RESOURCES[stage]
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = {f: f.path for f in [ctx.file._runner] + inputs},
        outputs = outputs,
        command = ["/bin/sh", "-c", 'cd /in && exec python3 -I "$@"', "runner", ctx.file._runner.path, stage, json.encode(params)],
        env = {
            "HOME": "/tmp",
            "LC_ALL": "C",
            "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch),
            "TZ": "UTC",
        },
        cpus = cpus,
        mem = mem,
        mnemonic = "Rockchip" + stage,
        progress_message = "Rockchip %s stage for %%{label}" % stage,
    )
    return [DefaultInfo(files = depset(outputs))]

def _source(ctx):
    return {
        "archive": ctx.file.archive.path,
        "patches": [f.path for f in ctx.files.patches],
        "patch_fuzz": ctx.attr.patch_fuzz,
    }

def _kbuild(ctx):
    return {"build_user": ctx.attr.build_user, "build_host": ctx.attr.build_host}

def _mapping(targets):
    entries = []
    for target, destination in targets.items():
        files = target.files.to_list()
        if len(files) != 1 or files[0].is_directory:
            fail("%s must provide exactly one file for %s" % (target.label, destination))
        entries.append([files[0].path, destination])
    return entries

def _kernel(ctx):
    params = _source(ctx) | _kbuild(ctx) | {
        "config": ctx.file.config.path,
        "dts_files": _mapping(ctx.attr.dts_files) + [
            [f.path, directory + "/" + f.basename]
            for target, directory in ctx.attr.dts_dirs.items()
            for f in target.files.to_list()
        ],
        "dtb_target": ctx.attr.dtb_target,
        "extra_dtb_targets": ctx.attr.extra_dtb_targets,
        "builtin_sources": _mapping(ctx.attr.builtin_sources),
    }
    if len(ctx.attr.extra_dtb_targets) != len(ctx.outputs.extra_dtbs):
        fail("extra_dtb_targets and extra_dtbs must have the same length")
    inputs = [ctx.file.archive, ctx.file.config] + ctx.files.patches + ctx.files.dts_files + ctx.files.dts_dirs + ctx.files.builtin_sources
    outputs = [ctx.outputs.image, ctx.outputs.dtb] + ctx.outputs.extra_dtbs
    return _stage(ctx, "Kernel", inputs, outputs, params)

rockchip_linux_kernel = rule(
    implementation = _kernel,
    doc = "Build an ARM64 Linux Image and a selected Kbuild DTB from a source archive.",
    attrs = _NATIVE | _SOURCE | _KBUILD | {
        "config": attr.label(mandatory = True, allow_single_file = True, doc = "Base .config for olddefconfig."),
        "dts_files": attr.label_keyed_string_dict(allow_files = True, doc = "Optional source labels mapped to paths under arch/arm64/boot/dts/."),
        "dts_dirs": attr.label_keyed_string_dict(allow_files = True, doc = "Labels whose files are installed by name into a directory under arch/arm64/boot/dts/."),
        "dtb_target": attr.string(mandatory = True, doc = "Kbuild DTB target relative to arch/arm64/boot/dts/."),
        "extra_dtb_targets": attr.string_list(doc = "Further Kbuild DTB or `.dtbo` overlay targets, built into `extra_dtbs` in order. DTBs keep their symbols for overlays."),
        "builtin_sources": attr.label_keyed_string_dict(allow_files = True, doc = "C sources, headers and firmware files mapped to source-tree paths. C files are linked into their directory as obj-y; files under firmware/ can be named in CONFIG_EXTRA_FIRMWARE with CONFIG_EXTRA_FIRMWARE_DIR=\"firmware\"."),
        "image": attr.output(mandatory = True),
        "dtb": attr.output(mandatory = True),
        "extra_dtbs": attr.output_list(),
    },
)

def _uboot(ctx):
    outputs = [ctx.outputs.idbloader, ctx.outputs.uboot, ctx.outputs.trust, ctx.outputs.script]
    inputs = [ctx.file.archive, ctx.file.defconfig, ctx.file.ddr, ctx.file.miniloader, ctx.file.bl31, ctx.file.trust_ini, ctx.file.boot_command]
    if ctx.file.trust_checksum:
        inputs.append(ctx.file.trust_checksum)
    params = _source(ctx) | _kbuild(ctx) | {
        "defconfig": ctx.file.defconfig.path,
        "ddr": ctx.file.ddr.path,
        "miniloader": ctx.file.miniloader.path,
        "bl31": ctx.file.bl31.path,
        "trust_ini": ctx.file.trust_ini.path,
        "trust_checksum": ctx.file.trust_checksum.path if ctx.file.trust_checksum else None,
        "boot_command": ctx.file.boot_command.path,
        "script_name": ctx.attr.script_name,
        "mkimage_soc": ctx.attr.mkimage_soc,
        "load_address": ctx.attr.load_address,
    }
    return _stage(ctx, "LegacyUBoot", inputs + ctx.files.patches, outputs, params)

rockchip_legacy_uboot = rule(
    implementation = _uboot,
    doc = "Build the Hardkernel-style AArch64 U-Boot, rksd DDR/miniloader, BL3X trust and legacy boot script.",
    attrs = _NATIVE | _SOURCE | _KBUILD | {
        "defconfig": attr.label(mandatory = True, allow_single_file = True),
        "ddr": attr.label(mandatory = True, allow_single_file = True),
        "miniloader": attr.label(mandatory = True, allow_single_file = True),
        "bl31": attr.label(mandatory = True, allow_single_file = True),
        "trust_ini": attr.label(mandatory = True, allow_single_file = True, doc = "trust_merger input using bl31.elf and output trust.img."),
        "trust_checksum": attr.label(allow_single_file = True, doc = "Optional expected SHA-256 for the complete trust.img."),
        "boot_command": attr.label(mandatory = True, allow_single_file = True),
        "script_name": attr.string(mandatory = True, doc = "Image name in the legacy script header (up to 32 ASCII bytes)."),
        "mkimage_soc": attr.string(mandatory = True, doc = "mkimage -n chip identifier for the rksd ID block."),
        "load_address": attr.string(mandatory = True, doc = "32-bit hex U-Boot payload address for loaderimage."),
        "idbloader": attr.output(mandatory = True),
        "uboot": attr.output(mandatory = True),
        "trust": attr.output(mandatory = True),
        "script": attr.output(mandatory = True),
    },
)

def _rootfs(ctx):
    params = _source(ctx) | {
        "config": ctx.file.config.path,
        "files": _mapping(ctx.attr.files),
        "executables": _mapping(ctx.attr.executables),
        "source_files": ctx.attr.source_files,
        "directories": ctx.attr.directories,
    }
    inputs = [ctx.file.archive, ctx.file.config] + ctx.files.patches + ctx.files.files + ctx.files.executables
    return _stage(ctx, "BusyBoxSquashfs", inputs, [ctx.outputs.out], params)

busybox_squashfs = rule(
    implementation = _rootfs,
    doc = "Build a static AArch64 BusyBox root filesystem with caller-supplied files and directories.",
    attrs = _NATIVE | _SOURCE | {
        "config": attr.label(mandatory = True, allow_single_file = True, doc = "BusyBox options over allnoconfig, including CONFIG_STATIC=y."),
        "files": attr.label_keyed_string_dict(allow_files = True, doc = "Data labels mapped to root-relative destinations, mode 0644."),
        "executables": attr.label_keyed_string_dict(allow_files = True, doc = "Executable labels mapped to root-relative destinations, mode 0755."),
        "source_files": attr.string_dict(doc = "Files from the unpacked BusyBox source mapped to root-relative destinations, mode 0644."),
        "directories": attr.string_list(doc = "Root-relative directories to create."),
        "out": attr.output(mandatory = True),
    },
)

def _disk(ctx):
    params = {
        "layout": ctx.file.layout.path,
        "inputs": _mapping(ctx.attr.inputs),
        "layout_image": ctx.attr.layout_image,
    }
    return _stage(ctx, "SDImage", [ctx.file.layout] + ctx.files.inputs, [ctx.outputs.out], params)

rockchip_sd_image = rule(
    implementation = _disk,
    doc = "Assemble a raw card file using an explicit genimage layout and named input payloads.",
    attrs = _NATIVE | {
        "layout": attr.label(mandatory = True, allow_single_file = True),
        "inputs": attr.label_keyed_string_dict(allow_files = True, doc = "Single-file labels mapped to paths in genimage's input directory."),
        "layout_image": attr.string(mandatory = True, doc = "Name of the final image entry in the layout."),
        "out": attr.output(mandatory = True),
    },
)

def rockchip_card_test(name, image, bl31, trust_checksum, boot_command, card, **kwargs):
    """Checks a legacy-boot card image in the build VM.

    `card` is a dict with `disk_signature`, `size_mib`, `partitions`,
    `idbloader`, `boot_files` and optional `device_tree`; see card_test.py.
    """
    vm_test(
        name = name,
        src = Label("//tools/bazel/rockchip:card_test.py"),
        data = [image, bl31, trust_checksum, boot_command],
        guest_args = [
            "$(rootpath %s)" % image,
            "$(rootpath %s)" % bl31,
            "$(rootpath %s)" % trust_checksum,
            "$(rootpath %s)" % boot_command,
            json.encode(card),
        ],
        **kwargs
    )
