# Gizlet launcher

The launcher on a Gizlet OS card: a home screen, settings and a button test, written in Rust with raylib. Its design follows [gizlet/gizlet](https://github.com/gizlet/gizlet)'s frontend; the Store and installed apps are not connected yet.

Pages are laid out in logical units: the screen is 720 units tall and at least 960 wide, so one layout fits the R36S's 640×480 panel, a 1280×720 window and a 4K monitor; wider screens get more width. [Painter](src/painter.rs)

| Page | What it does |
| --- | --- |
| Home | The app shelf (empty for now), Library, Store, Button Test, Settings and Power |
| Settings | Wi-Fi on/off, scan, join with an on-screen keyboard, forget; SSH on/off with the login address and key count; brightness and theme; volume; battery; A/B swap; language (中文/English); date and time: the time zone, whether network time has set the clock, and the 24-hour clock; device information |
| Button test | A handheld drawn from primitives that lights up with every button, stick and trigger; hold Back to leave |

On Gizlet OS it reads the handheld's controls straight from evdev (BTN_EAST is A, BTN_SOUTH is B), and the volume keys change the volume. Wi-Fi and ssh are switched and the time zone is set with `gizlet-switch`, which keeps the choice across reboots; the clock redraws in a new zone at once. Missing hardware or drivers are shown as such, as for the R36S's battery and sound today. [System](src/system/linux.rs) · [Controls](src/evdev.rs)

## Running on a computer

```sh
cd launcher
GIZLET_FAKE_DEVICES=1 cargo run --release
```

Keys: arrows, Enter or K (A), Escape or J (B), I (X), U (Y), Q/E (L/R), = (Start), - (Select). `GIZLET_WINDOW=640x480` sets the window size, `GIZLET_FAKE_DEVICES=1` simulates the battery, backlight, volume, Wi-Fi and ssh, and `GIZLET_SCRIPT` drives it from a script (see [script.rs](src/script.rs)).

## Building for the card

```sh
bazel build --config=r36s //launcher:gizlet-launcher
```

On the card the launcher draws with OpenGL ES 2 through Gizlet runtime 1's raylib and Mesa: Panfrost on the R36S's GPU, llvmpipe on the CPU where there is no GPU driver. It links the runtime's `libraylib.so` (`//runtime:raylib`, built from the same vendored raylib-sys sources as its bindings), and [start.sh](start.sh), installed as its boot entry, runs it through the runtime's `gizlet-run`. It redraws every frame. [Build](build_card.sh) · [Runtime](../runtime/README.md)

## Fonts

Titles use Space Grotesk and text uses Inter, both under the SIL Open Font License; Chinese comes from Droid Sans Fallback under the Apache License 2.0. Their licenses are in [fonts](fonts) and installed under `/usr/share/licenses/gizlet-launcher`.
