# Gizlet launcher

The launcher on a Gizlet OS card: a home screen with the installed apps, settings and a button test, written in Rust with raylib. Its program also carries the `gizlet` command that installs and runs apps. Its design follows [gizlet/gizlet](https://github.com/gizlet/gizlet)'s frontend; the Store is not connected yet.

Pages are laid out in logical units: the screen is 720 units tall and at least 960 wide, so one layout fits the R36S's 640×480 panel, a 1280×720 window and a 4K monitor; wider screens get more width. [Painter](src/painter.rs)

| Page | What it does |
| --- | --- |
| Home | The shelf of installed apps (A starts one), Library, Store, Button Test, Settings and Power |
| Settings | Wi-Fi on/off, scan, join with an on-screen keyboard, forget; SSH on/off with the login address and key count; brightness and theme; volume; battery; A/B swap; language (中文/English); date and time: the time zone, whether network time has set the clock, and the 24-hour clock; device information |
| Button test | A handheld drawn from primitives that lights up with every button, stick and trigger; hold Back to leave |

On Gizlet OS it reads the handheld's controls straight from evdev (BTN_EAST is A, BTN_SOUTH is B), and the volume keys change the volume. Wi-Fi and ssh are switched and the time zone is set with `gizlet-switch`, which keeps the choice across reboots; the clock redraws in a new zone at once. Missing hardware or drivers are shown as such, as for the R36S's battery and sound today. [System](src/system/linux.rs) · [Controls](src/evdev.rs)

## Apps

An app is installed from a `.gzl` package, the format of the [Gizlet store contract](https://github.com/gizlet/store/blob/main/docs/contract.md): a zip with `manifest.toml` at its root and `CHECKSUMS`, the SHA-256 of every other file. The store's release index, `gizlet.json`, names the packages of a release; the device reads only the package. [Packages](src/apps.rs)

```toml
id = "io.gizlet.overeaten"
name = "Overeaten"
version = "0.1.0"
tier = "lite"
target = "aarch64-unknown-linux-gnu"
entry = "bin/overeaten"
icon = "icon.png"
color = "#c8553d"

[names]
zh-CN = "吃吃吃"
```

`id`, `name`, `version`, `tier`, `target` and `entry` are required; `icon`, `color`, `description`, `[names]` and `[descriptions]` are optional. Installing checks what the store checks: stored or deflated entries with canonical relative names, no links, devices, setuid files, encryption or ZIP64, every file matching `CHECKSUMS`, a reverse-DNS `id`, and a `target` and `tier` this device runs (`aarch64-unknown-linux-gnu` or `-musl`, `lite`). The package is unpacked beside the installed version and replaces it only when complete.

| Path under `/opt/gizlet` | Holds |
| --- | --- |
| `apps/<id>/` | The installed package |
| `saves/<id>/` | The app's own files, kept across updates and, unless asked, uninstalls |
| `saves/.played` | When each app last ran, for the shelf's order |

An app runs with its folder as the working directory and `GIZLET_APP_ID`, `GIZLET_APP_DIR`, `GIZLET_SAVE_DIR`, `GIZLET_LANG` (`zh-CN` or `en`, from the launcher's language) and `GIZLET_SWAP_AB` (`1` or `0`). A dynamically linked entry runs through Gizlet runtime 1's `gizlet-run`, so it gets the runtime's glibc, raylib and Mesa; scripts and static executables run as they are.

The launcher hands the screen to an app by writing its ID to `/run/gizlet-launch` and quitting. [start.sh](start.sh), the boot entry, then runs the app and starts the launcher again when it exits. On a computer the app opens beside the launcher.

`/usr/bin/gizlet` runs these commands from ssh or the recovery shell; elsewhere they are subcommands of `gizlet-launcher`, and `GIZLET_PREFIX` replaces `/opt/gizlet`. [Commands](src/cli.rs)

| Command | Does |
| --- | --- |
| `gizlet install FILE.gzl` | Installs or updates an app; refused while that app runs |
| `gizlet uninstall ID [--saves]` | Removes it, and its saves with `--saves`; refused while it runs |
| `gizlet list` | Lists installed apps: ID, version and name |
| `gizlet run ID [ARGS]` | Runs it here and waits; the screen must be free, as in the recovery shell |
| `gizlet start ID` | Asks the launcher on the screen to quit and run it, then come back |
| `gizlet target` | Prints the target triple and tier packages must match |

## Running on a computer

```sh
cd launcher
GIZLET_FAKE_DEVICES=1 cargo run --release
```

Apps installed into the launcher's data folder (`GIZLET_PREFIX` to choose another) appear on its shelf; `GIZLET_TARGET=aarch64-unknown-linux-gnu` lets a Mac install a handheld package to see its tile.

Keys: arrows, Enter or K (A), Escape or J (B), I (X), U (Y), Q/E (L/R), = (Start), - (Select). `GIZLET_WINDOW=640x480` sets the window size, `GIZLET_FAKE_DEVICES=1` simulates the battery, backlight, volume, Wi-Fi and ssh, and `GIZLET_SCRIPT` drives it from a script (see [script.rs](src/script.rs)).

## Building for the card

```sh
bazel build --config=r36s //launcher:gizlet-launcher
```

On the card the launcher draws with OpenGL ES 2 through Gizlet runtime 1's raylib and Mesa: Panfrost on the R36S's GPU, llvmpipe on the CPU where there is no GPU driver. It links the runtime's `libraylib.so` (`//runtime:raylib`, built from the same vendored raylib-sys sources as its bindings), and [start.sh](start.sh), installed as its boot entry, runs it through the runtime's `gizlet-run`. It redraws every frame. [Build](build_card.sh) · [Runtime](../runtime/README.md)

## Fonts

Titles use Space Grotesk and text uses Inter, both under the SIL Open Font License; Chinese comes from Droid Sans Fallback under the Apache License 2.0. Their licenses are in [fonts](fonts) and installed under `/usr/share/licenses/gizlet-launcher`.
