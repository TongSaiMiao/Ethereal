# Ethereal agent rules

## Manager must always open

The manager is a normal Android app. A user with **no root**, **no SuperCall**, and **no kernel module** must still be able to launch it and land on Home.

- Home always shows status cards.
- If the kernel module is absent: show **未安装 / Not installed** (kernel card + module-service card). Never crash, never `exitProcess`, never a blank/splash-stuck screen.
- If the kernel module is present but userspace root failed: show kernel working and module-service **未安装**. Still never crash.
- SuperUser / module tabs stay hidden until the matching state is actually working (`kernelRequired` / `serviceRequired`).

Failing to get root is a **UI state**, not a fatal error. It must not take down rooted users either.

## Startup is unprivileged

`Application.onCreate` and the first frame of `MainActivity` may only do ordinary app work (prefs, theme, compose, copy assets). They must **not**:

- call SuperCall (`reboot` magic / `ethereal_fd` / `Natives.su` / `nativeReady`)
- `Runtime.exec` / libsu `Shell.build` of `su`, `sh`, or any root helper
- load `libetherealjni.so` unless a later background probe needs it

ColorOS treats exec of `su` as a root-permission prompt and **kills the process on deny**. The same `reboot` SuperCall from the UI process can be seccomp `SIGSYS`-killed. Either path looks like a random crash to both rooted and unrooted users.

Detect the module with Java `/sys/module/ethereal` **after** the Home UI is on screen. Only then, on a background thread, optionally load JNI and try SuperCall.

## SuperCall must not kill the UI process

JNI SuperCall (`reboot` -> anon fd -> ioctl) must run in a **forked child**. If the child is signal-killed, the parent keeps running and SuperCall just returns failure. Never issue `reboot()` on the main thread.

Do not use `@FastNative` for SuperCall entry points.

## Root helper rules

- Never exec a binary named `su` from the app sandbox.
- After SuperCall has made **this process** uid 0, `/system/bin/sh` is allowed.
- `installEthereal` / file copies use Java `File` APIs once uid is 0. Do not spawn libsu just to `mkdir`/`cp`.
- SuperUser app list uses `PackageManager` (`QUERY_ALL_PACKAGES`). Do not block that screen on RootService / libsu.
- Patching user-selected `boot.img` / `init_boot.img` files is an offline, unprivileged flow. Only direct partition reads and writes may require SuperCall root.

## Do not regress this

If a change makes the app unable to open without root, or crash when a ColorOS root dialog is dismissed, it is a blocker. Show the not-installed Home instead.
