# Gizlet launcher

The launcher on a Gizlet OS card: a home screen with the installed apps, settings and a button test, written in Rust with raylib. Its program also carries the `gizlet` command that installs and runs apps and installs and controls background services. Its design follows [gizlet/gizlet](https://github.com/gizlet/gizlet)'s frontend; the Store is not connected yet.

Pages are laid out in logical units: the screen is 720 units tall and at least 960 wide, so one layout fits the R36S's 640×480 panel, a 1280×720 window and a 4K monitor; wider screens get more width. [Painter](src/painter.rs)

| Page | What it does |
| --- | --- |
| Home | The shelf of installed apps (A starts one), Library, Store, Button Test, Settings and Power |
| Settings | Wi-Fi on/off, scan, join with an on-screen keyboard, forget; installed services: running or stopped, whether each starts at boot, and its state; SSH on/off with the login address (`NAME.local` and the IP address) and key count; brightness and theme; volume and interface sounds; battery: charge, whether it is charging, power draw and the time left at that draw (hidden below 50 mA, where the gauge reads noise), separate sleep times on battery and while charging, and the CPU and GPU clocks and CPU use behind it; A/B swap; language (中文/English); date and time: the time zone, whether network time has set the clock, and the 24-hour clock; device information and the hostname, which can be changed |
| Button test | A handheld drawn from primitives that lights up with every button, stick and trigger; hold Back to leave |

On Gizlet OS it reads the handheld's controls straight from evdev (BTN_EAST is A, BTN_SOUTH is B), and the volume keys change the volume. Wi-Fi and ssh are switched and the time zone and hostname are set with `gizlet-switch`, which keeps the choice across reboots; the clock redraws in a new zone at once. Missing hardware or drivers are shown as such, as for the R36S's battery and sound today. [System](src/system/linux.rs) · [Controls](src/evdev.rs)

Brightness and volume change on screen at once and reach the device on the system worker. A device report read before the latest change keeps the value shown, so a slider does not jump back while the device catches up.

Where the mixer control reports its decibel range, the volume moves along decibels: 0% is silent, and 5% to 100% run evenly up to 10 dB below the control's maximum, across at most 40 dB. On the R36S, whose `Master` spans -95 dB to 0 dB, that is -50 dB to -10 dB, 2 dB a step. Its raw percentage left half the slider inaudible, and its speaker distorts at full scale. Controls without a range, or with less than 12 dB to use, are set by percentage. [Volume](src/system/linux.rs)

Moving the focus, opening or closing a page, menu or keyboard, and changing a setting play short tones that the launcher synthesizes; Settings → Sound turns them off. Every volume change, from the slider or the volume keys, plays a tone once the device has the new level, so it is heard at that level. The audio device opens for a sound and closes after eight seconds of quiet, so an idle launcher keeps no audio running. [Sounds](src/sound.rs)

## Apps

Developers write `gizlet.toml` at the repository root. The store reads that file at the release tag, and the build generates `manifest.json` for each `.gzl`. The device reads the manifest from the package root, where `CHECKSUMS` lists the SHA-256 of every other file. The store also reads the manifest when it checks the package. [Gizlet format](https://github.com/gizlet/gizlet/blob/main/docs/gizlet-toml.md) · [Package reader](src/apps.rs)

```json
{
  "schema": 1,
  "id": "io.gizlet.overeaten",
  "name": "Overeaten",
  "names": { "zh-CN": "吃吃吃" },
  "version": "0.1.0",
  "runtime": 1,
  "target": "aarch64-unknown-linux-gnu",
  "tier": "lite",
  "entry": "bin/overeaten",
  "icon": "icon.png",
  "color": "#c8553d"
}
```

`id`, `name`, `version`, `tier`, `target` and `entry` are required. The app shelf uses the package's `icon`, `color` and localized `names` when provided. `description` and its `descriptions` translations are also optional. The OS supports runtime `1` and assumes it when omitted.

`schema` says how to read the manifest, and with it the package's kind:

| `schema` | `kind` | The OS treats it as |
| --- | --- | --- |
| `1`, or left out | Left out, or `app` | An app: every package built before schema 2 |
| `2` | `app` | An app, exactly like schema 1 |
| `2` | `service` | A service: a background program that systemd runs. It is never shown on the shelf or started by `gizlet run` or `gizlet start`. See [Services](#services) |

Any other schema, a schema 2 manifest without `kind`, and a schema 1 manifest whose `kind` is not `app` are refused. [Manifest contract](https://github.com/gizlet/gizlet/blob/main/docs/gizlet-toml.md#manifestjson)

The installer accepts stored or deflated entries with canonical relative names. It rejects links, devices, setuid files, encryption and ZIP64, verifies every file against `CHECKSUMS`, and checks the reverse-DNS `id` and whether the device supports the package's `target` and `tier` (`aarch64-unknown-linux-gnu` or a static `-musl` program, and `lite` on the R36S). The package is unpacked beside the installed version and replaces it only when complete.

| Path under `/opt/gizlet` | Holds |
| --- | --- |
| `apps/<id>/` | The installed package |
| `saves/<id>/` | The app's own files, kept across updates and, unless asked, uninstalls |
| `saves/.played` | When each app last ran, for the shelf's order |

An app runs with its folder as the working directory and `GIZLET_APP_ID`, `GIZLET_APP_DIR`, `GIZLET_SAVE_DIR`, `GIZLET_LANG` (`zh-CN` or `en`, from the launcher's language) and `GIZLET_SWAP_AB` (`1` or `0`). A dynamically linked entry runs through Gizlet runtime 1's `gizlet-run`, so it gets the runtime's glibc, raylib and Mesa; scripts and static executables run as they are.

The launcher hands the screen to an app by writing its ID to `/run/gizlet-launch` and quitting. [start.sh](start.sh), the boot entry, then runs the app and starts the launcher again when it exits. On a computer the app opens beside the launcher.

Every app can be left the same way: holding Home (the R36S's FN key) for a second, or Select and Start together for two seconds, sends it SIGTERM, then SIGKILL three seconds later if it is still running, and the launcher comes back. `gizlet run` asks every input device which buttons it holds down (`EVIOCGKEY`) ten times a second, so an app that grabs its controls cannot hide these. Apps that keep progress should save it on SIGTERM. [Exit](src/supervise.rs)

While neither holds the display, the LCD shows the kernel's text console. start.sh clears it and hides its cursor, and after boot the kernel prints only critical messages there (`/usr/lib/sysctl.d/gizlet.conf`), so a switch shows black until the app draws its first frame.

On Gizlet OS, `/usr/bin/gizlet` runs the CLI from the Launcher release selected at boot. On a computer, run `cargo run --release --bin gizlet-cli -- COMMAND` from `launcher/`. Set `GIZLET_PREFIX` to choose where apps and saves live. [Commands](src/cli.rs)

| Command | Does |
| --- | --- |
| `gizlet install FILE.gzl` | Installs or updates an app, refused while that app runs; or a service (see [Services](#services)) |
| `gizlet uninstall ID [--saves]` | Removes an app, and its saves with `--saves`; refused while it runs. Stops and removes a service with everything it keeps |
| `gizlet list` | Lists installed apps: ID, version and name |
| `gizlet run ID [ARGS]` | Runs it here and waits; the screen must be free, as in the recovery shell |
| `gizlet start ID` | Asks the launcher on the screen to quit and run it, then come back |
| `gizlet target` | Prints the target triple and tier packages must match |
| `gizlet service list` | Lists installed services: ID, version, state, `enabled` or `disabled` at boot, and name |
| `gizlet service status ID` | Shows a service's state and process, whether it starts at boot, its unit and its folders |
| `gizlet service start\|stop\|restart ID` | Starts or stops a service now; stopping ends every process it started |
| `gizlet service enable\|disable ID` | Chooses whether a service starts at boot, without starting or stopping it |
| `gizlet service logs ID [-f]` | Shows a service's output from the journal, or follows it |
| `gizlet system reload-runtime FILE.sqfs` | Activates a local runtime 1 image for new launches on Gizlet OS as root |

See [Activating a new build](../runtime/README.md#activating-a-new-build) for runtime updates.

## Services

A service is a background program with no screen: a schema 2 package with `"kind": "service"`. It is built, published and installed like an app, with `gizlet install FILE.gzl`, but it runs under systemd rather than from the shelf, so it keeps running while the launcher hands the screen to an app, comes back or restarts. Settings → Services and `gizlet service` show and control it. [Services](src/services.rs) · [Commands](src/cli.rs)

| Path | Holds |
| --- | --- |
| `/opt/gizlet/services/<id>/` | The installed package |
| `/opt/gizlet/services/.boot-<id>` | Present while it starts at boot |
| `/opt/gizlet/service-data/<id>/` | Its state and configuration, kept across updates, deleted on uninstall |
| `/run/gizlet-services/<id>/` | Its sockets and other runtime files: created as it starts, removed when it stops |
| `/run/systemd/system/gizlet-service-<id>.service` | Its systemd unit, written by `gizlet` |

The read-only base cannot keep units in `/etc`, so `gizlet` writes each service's unit to `/run/systemd/system`, and `gizlet-services.service` (`gizlet service boot`) writes them again on every boot, after the data partition is mounted. It then starts the services set to start at boot, whatever `boot.ini` starts. On a base from before services, which gets this Launcher over the air, the Launcher's start script does the same once per boot; a custom `boot.ini` entry needs the card writer's base update first.

### For service authors

The unit runs `gizlet service exec <id>`, which replaces itself with the package's `entry`, as root, with no terminal or display. A dynamically linked entry runs through Gizlet runtime 1's `gizlet-run`, as an app's does. It gets:

| Environment | Value |
| --- | --- |
| `GIZLET_SERVICE_ID` | Its ID |
| `GIZLET_SERVICE_DIR` | The package folder, also the working directory; treat it as read-only, since an update replaces it |
| `GIZLET_STATE_DIR` | `/opt/gizlet/service-data/<id>`, mode 0700: state and configuration that must survive updates and reboots. `HOME` points here too |
| `GIZLET_RUNTIME_DIR` | `/run/gizlet-services/<id>`, mode 0700: sockets, PID and lock files. Empty at every start |

- Keep everything the service writes in those two folders. `/tmp` is private to the service and emptied when it stops. Other paths are outside the package, and uninstalling does not remove them.
- Write logs to standard output or error; they go to the journal (`gizlet service logs ID`).
- Stay in the foreground. A crash restarts the service two seconds later, up to five times a minute. Exiting with status 0 stops it without a restart.
- Stopping, updating and uninstalling send SIGTERM to the service and every process it started, then SIGKILL after 15 seconds. Shut down within that time and release what the service holds.
- Installing a service starts it and sets it to start at boot. An update stops the running version, replaces the package, and starts the new version if the old one ran, keeping the state and the boot choice. A new version that exits, even with status 0, or fails within three seconds is replaced by the previous one, which starts again; a new service that does not start is not installed. The journal keeps the failure.

### Uninstalling

`gizlet uninstall ID` first turns off start at boot, then stops the service and every process in its control group and checks that none remain. It then removes the unit and deletes the package, the state folder and the runtime folder. Apps, their saves, other services and system services are untouched. If a step fails, it stops there and says what is left and what to do. If the service cannot be stopped, nothing is deleted, and it no longer starts at boot: restart the device, then uninstall again. If a file cannot be deleted, the service is already stopped, unregistered and no longer listed: fix the file and uninstall again. Until then it cannot be installed again. If an update is cut short before its new version is confirmed to start, the next `gizlet` command or boot puts the previous version back, and starts it again if it was running earlier in the same boot. An uninstall cut short is finished the same way.

## Updating the Launcher and CLI

On an OTA-capable card, open **Settings → System → System updates**. The Launcher shows its release version and the SHA-256 prefix of the running Runtime 1 build. Check for updates, confirm the download, then restart when it is ready. The new Runtime and Launcher with its CLI start together. The system keeps the previous working pair until the new Launcher draws its first frame. A failed or interrupted trial restores the previous pair and shows the rolled-back release in Settings.

Cards from before OTA support need one card-writer **Update** first. That updates the base with the boot recovery logic and preserves the data partition. OTA updates the Runtime and Launcher on that partition. The card writer still updates the base, kernel and boot files. With a custom `boot.ini`, the update trial waits until the default Launcher entry runs.

Use `gizlet system ota status`, `gizlet system ota check`, and `gizlet system ota install TAG` for the same flow over SSH.

### Manual Launcher bundles

A Launcher release contains the UI, its start script, and `bin/cli`. On boot, Gizlet OS picks the highest complete, compatible `MAJOR.MINOR.PATCH` release under `/opt/gizlet/launcher/releases`, or the factory release from the read-only rootfs. The selection lasts for that boot. A custom `boot.ini` still controls the boot entry. `gizlet` uses the selected CLI in either case.

Build both card binaries and use an OpenSSH release signing key to pack them:

```sh
bazel build --config=r36s //launcher:gizlet-launcher //launcher:gizlet-cli
./launcher/pack_release.sh 0.2.0 /path/to/signing-key bazel-bin/launcher/gizlet-launcher bazel-bin/launcher/gizlet-cli gizlet-launcher-0.2.0.gizlet
```

Release images include the official Launcher signing public key. To use a different signing key or update an older image, put a line of the form `launcher namespaces="gizlet-launcher" ssh-ed25519 KEY` in `/opt/gizlet/launcher/allowed_signers`, replacing `ssh-ed25519 KEY` with the release public key's type and value. This file takes precedence over the built-in key.

Copy the signed `.gizlet` file to the device with `scp` and pass its path to `gizlet update`. For an over-the-air update, publish the same bundle at an HTTPS URL and pass that URL instead:

```sh
gizlet update /home/gizlet-launcher-0.2.0.gizlet
```

The installer verifies the signature and architecture and runtime requirements, then publishes the complete release with a rename. Restart to use the newest release. The system keeps the three most recent complete releases, plus the running release until the next boot if needed. Apps, saves, and settings live outside these releases.

## Running on a computer

```sh
cd launcher
GIZLET_FAKE_DEVICES=1 cargo run --release
```

Apps installed into the launcher's data folder (`GIZLET_PREFIX` to choose another) appear on its shelf; `GIZLET_TARGET=aarch64-unknown-linux-gnu` lets a Mac install a handheld package to see its tile.

Keys: arrows, Enter or K (A), Escape or J (B), I (X), U (Y), Q/E (L/R), = (Start), - (Select). `GIZLET_WINDOW=640x480` sets the window size, `GIZLET_FAKE_DEVICES=1` simulates the battery, backlight, volume, Wi-Fi and ssh, and `GIZLET_SCRIPT` drives it from a script (see [script.rs](src/script.rs)).

## Building for the card

```sh
bazel build --config=r36s //launcher:gizlet-launcher //launcher:gizlet-cli
```

On the card the launcher draws with OpenGL ES 3.0 through Gizlet runtime 1's raylib and Mesa: Panfrost on the R36S's GPU, llvmpipe on the CPU where there is no GPU driver. It links the runtime's `libraylib.so` (`//runtime:raylib`, built from the same vendored raylib-sys sources as its bindings), and [start.sh](start.sh), installed as its boot entry, runs it through the runtime's `gizlet-run`. It draws 60 frames a second while anything moves. Four seconds after the last input the focus stops pulsing and the screen stands still: it is drawn again only for input, a device report or the clock, about once a second, so an idle launcher leaves the CPU and GPU asleep. On the R36S this cut the idle draw from about 790 to 530 mA. [Build](build_card.sh) · [Runtime](../runtime/README.md)

## Fonts

Titles use Space Grotesk and text uses Inter, both under the SIL Open Font License; Chinese comes from Droid Sans Fallback under the Apache License 2.0. Their licenses are in [fonts](fonts) and installed under `/usr/share/licenses/gizlet-launcher`.
