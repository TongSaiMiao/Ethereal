# Third-Party Notices

This file records the externally sourced code and binaries shipped by
Ethereal. Those components keep their upstream copyright and license terms.

## Magisk policy engine

The policy engine under `ethd/vendor/magiskpolicy` is based on Magisk commit
`8903cf7f2261fb60bd2d0b568d6becb2ecf74c3e`. Its bundled `libsepol` comes from
the `topjohnwu/selinux` commit
`be1b39a657fee7faacfae548b75cb53302043a01`. The local source changes and exact
links are documented in `ethd/vendor/magiskpolicy/UPSTREAM.md`.

The Magisk policy code is GPL-3.0-only. The retained `libsepol` files carry
their own LGPL, GPL, BSD, and other upstream notices. Local license copies are
in `ethd/vendor/magiskpolicy/LICENSE` and
`ethd/vendor/magiskpolicy/libsepol/LICENSE`.

## BusyBox

`app/libs/arm64-v8a/libbusybox.so` is the unchanged ARM64 BusyBox binary from
the official Magisk v28.0 APK. Magisk v28.0 points to commit
`28cccdf7aa49356981fb490c440b31d70326d884`. The binary identifies itself as
`BusyBox v1.36.1.1 topjohnwu` and has this SHA-256 digest:

```text
4d60ab3f5a59ebb2ca863f2f514e6924401b581e9b64f602665c008177626651
```

Corresponding upstream source inputs are BusyBox 1.36.1 tag commit
`1a64f6a20aaf6ea4dbba68bbfa8cc1ab7e5c57c4` and topjohnwu's Android build
patches at `topjohnwu/ndk-box-kitchen` commit
`14d189ea3070a8167b3576bf83fe070d4a3441af`.

BusyBox is GPL-2.0-only. Its license is included at
`third_party/busybox/LICENSE`.

## KernelSU

The Manager UI and module conventions include work from KernelSU. KernelSU's
source and notices are available at <https://github.com/tiann/KernelSU>.
