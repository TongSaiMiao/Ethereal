<div align="center">
  <p>
    <strong>English</strong> ·
    <a href="README_zh-CN.md">简体中文</a> ·
    <a href="README_zh-TW.md">繁體中文</a>
  </p>
  <img
    src="app/src/main/res/drawable-nodpi/ic_launcher_monochrome.png"
    alt="Ethereal app icon"
    width="120"
  />
  <h1>Ethereal</h1>
  <p><strong>Ramdisk-delivered, KMI-matched kernel-module root for ARM64 Android GKI devices.</strong></p>
  <p>
    <a href="https://github.com/TongSaiMiao/Ethereal/actions/workflows/build.yml">
      <img
        src="https://github.com/TongSaiMiao/Ethereal/actions/workflows/build.yml/badge.svg?branch=main"
        alt="Build Manager"
      />
    </a>
    <img
      src="https://img.shields.io/badge/platform-Android%20ARM64-3DDC84?logo=android&logoColor=white"
      alt="Android ARM64"
    />
  </p>
  <p>
    <a href="#compatibility">Compatibility</a> ·
    <a href="#installation">Installation</a> ·
    <a href="#building">Building</a> ·
    <a href="#documentation-and-support">Documentation</a> ·
    <a href="#license">License</a>
  </p>
</div>

Ethereal is a root implementation for Android devices running a supported
GKI 1.0 or GKI 2.0 kernel. It puts one module per Android Kernel Module
Interface (KMI) in the generic ramdisk, loads the exact matching
`ethereal.<kmi>.ko` during early boot, and then hands control back to Android.

The kernel `Image` payload is never rewritten. A GKI 1.0 file patch adds an
`rdinit=/ethereal-init` trampoline to one `boot.img` and leaves `/init`
untouched. A GKI 2.0 file patch takes one `init_boot.img`, keeps the original
`/init` as `init.ethereal.bak`, and redirects its ELF entry through an added
loader `PT_LOAD`; `boot.img` and its command line remain untouched.

> [!CAUTION]
> Flashing boot-critical images can make a device unbootable and may lead to
> data loss during recovery. Back up your data and the exact stock images for
> the current firmware and slot. Do not continue unless you have a tested path
> back to the bootloader, fastboot, or the device's OEM recovery tool.

## Highlights

- **Preserves the kernel payload.** Ethereal modifies boot metadata and the
  generic ramdisk; it does not rewrite the kernel `Image` binary.
- **Preserves the original init.** GKI 1.0 leaves `/init` untouched. GKI 2.0
  keeps an exact backup, runs `ethereal-init`, and then enters the original
  first-stage init code.
- **Matches the Android KMI exactly.** Every supported KMI has its own KO with
  symbol-version CRCs. An unknown or ambiguous KMI is never assigned a generic
  fallback.
- **Fails open to the stock boot path.** If no matching KO can be selected or
  loaded, Ethereal skips the module and continues to the stock `/init`.
- **Authenticates the Manager.** Manager-only operations require both the
  Manager's Android UID and a private 32-byte token generated on first launch.
- **Keeps the Manager usable without root.** It can display installation state
  and patch user-selected image files without privileged partition access.
- **Includes userspace management.** The Manager provides per-app superuser
  policy and a Magisk-style module lifecycle backed by `ethd`.

Android Manager package ID: `me.ethereal.app`

## How it works

The stock kernel must allow loading modules (`CONFIG_MODULES`). GKI 1.0 uses
the `rdinit=/ethereal-init` trampoline in one patched `boot.img`. Offline GKI
2.0 patching accepts one `init_boot.img`, injects the loader into the stock
`/init`, and leaves `boot.img` and its cmdline unchanged; a kernel-only GKI 2.0
`boot.img` is not a standalone patch target. Direct Install still updates the
matching `init_boot` and `boot` partitions as one transaction.

```text
Bootloader
└── stock kernel Image
    ├── GKI 1.0 / Direct Install: rdinit=/ethereal-init
    │   ├── identify the running Android KMI
    │   ├── load ethereal.<kmi>.ko with finit_module()
    │   └── execute the stock /init
    └── GKI 2.0 file patch: hooked /init ELF entry
        ├── run ethereal-init and load the exact KMI module
        └── enter the original first-stage init
            └── Android → Ethereal Manager ↔ authenticated SuperCall
                                      └── ethd module service
```

### Boot image changes

| Flow | Images patched | Changes |
| --- | --- | --- |
| GKI 1.0 file patch | One `boot.img` | Adds the loader, KMI modules, credentials, and private `su` payload to the ramdisk; adds `rdinit=/ethereal-init` to this image's command line. |
| GKI 2.0 file patch | One `init_boot.img` | Adds the same payload, backs up the effective root `/init`, and hooks its ELF entry. It does not read or modify `boot.img`. |
| GKI 2.0 Direct Install / inactive slot | Matching `init_boot` + `boot` partitions | Adds the payload to `init_boot` and `rdinit=/ethereal-init` to `boot`; both outputs are staged and published as one transaction. |

The file picker accepts exactly one image and detects its layout after
unpacking it. A kernel-only `boot.img`, `vendor_boot.img`, ambiguous image, or
already foreign-patched image is rejected instead of guessed. AOSP documents
`init_boot` as a partition-layout change for devices launching with Android 13;
devices upgraded from an older launch version can keep their generic ramdisk
in `boot`. See
[AOSP's generic boot partition documentation](https://source.android.com/docs/core/architecture/partitions/generic-boot).

## Compatibility

Ethereal currently bundles the following ARM64 modules:

| GKI generation | Android KMI | Kernel |
| --- | --- | --- |
| GKI 1.0 | `android12-5.4` | 5.4 |
| GKI 2.0 | `android12-5.10` | 5.10 |
| GKI 2.0 | `android13-5.10` | 5.10 |
| GKI 2.0 | `android13-5.15` | 5.15 |
| GKI 2.0 | `android14-5.15` | 5.15 |
| GKI 2.0 | `android14-6.1` | 6.1 |
| GKI 2.0 | `android15-6.6` | 6.6 |
| GKI 2.0 | `android16-6.12` | 6.12 |

The Android version in a KMI name identifies the kernel branch, not necessarily
the Android version currently running on the device. A matching major kernel
version alone is not sufficient. Ethereal identifies the KMI from the kernel
release and still enforces module symbol-version CRCs.

Every row in the table has a committed prebuilt KO and QEMU coverage against an
exact Google GKI image. This validates the release module against that GKI
build; it does **not** prove compatibility with every OEM vendor kernel derived
from the same KMI.

The Manager APK requires Android 8.0 or later (API 26) and ARM64, while the
bundled root payload is limited to the KMIs listed above.

### Requirements

- An ARM64 device with a supported, unambiguously identifiable Android KMI.
- A stock kernel built with `CONFIG_MODULES=y` that permits external module
  loading.
- Clean stock boot images from the exact current firmware and slot.
- A way to flash and restore boot partitions, normally an unlocked bootloader
  or an already working privileged installation path.
- A complete backup of important data before patching or flashing.

### Known limitations

- OEM kernel changes, module-signing policy, CFI, or extra platform protections
  can still reject a KO that works on the corresponding Google GKI image.
- Samsung devices with additional OEM kernel protections are currently
  untested and unsupported.
- Ethereal's module boot path is skipped when Magisk is detected. System
  overlays also require a suitable OverlayFS/metamodule setup.
- Ethereal does not include Zygisk. Ethereal module ZIPs must be installed from
  the Manager and are not supported in recovery.

## Installation

Signed tagged builds are published through
[GitHub Releases](https://github.com/TongSaiMiao/Ethereal/releases). If no tagged
release is available, build the Manager from source. Non-tag Actions artifacts
are development builds and should not be treated as signed releases.

> [!IMPORTANT]
> Open the Manager at least once **before** patching. It creates a private
> authentication token and embeds that token together with the Manager UID in
> each patched ramdisk. Never publish or share patched images. After flashing,
> do not clear the Manager's app data, uninstall and reinstall it, or switch to
> a differently signed build; doing so can invalidate Manager access and
> require a fresh patch from clean stock images. Normal in-place updates retain
> the app data.

### First installation

1. Install and open the Ethereal Manager.
2. Back up the clean stock image for the current firmware and slot. Do not use
   an image already modified by Ethereal, Magisk, or another patcher.
3. Open **Home → Install** and choose the file-based patch option. This flow
   does not require root and does not touch a physical partition.
4. Select exactly one stock image: `boot.img` when the generic ramdisk is in
   boot, or `init_boot.img` when the device has a separate init_boot partition.
5. Check the patch log. One `Ethereal-<original-filename>` output is written to
   `Downloads`.
6. Flash the output back to the same partition type. Partition names and A/B
   slot handling are OEM-specific; never flash an `init_boot` image to `boot`.
7. Reboot and open the **same Manager installation**. After the kernel module
   and Manager credential are verified, the Manager deploys the `ethd`
   userspace service and enables the applicable SuperUser and Modules tabs.

**Direct install** and **Install to inactive slot** require an already working,
authenticated Ethereal installation. They are intended for updates, repairs,
and post-OTA use—not for the first installation. On GKI 2.0 these privileged
flows patch the matching `init_boot` and `boot` partitions together. Use the
inactive-slot option only after the OTA has completed.

### Removal and recovery

The Manager's **Uninstall** action removes only the userspace module service.
It does not remove `rdinit` or `ethereal.ko` from the flashed images.

To remove Ethereal completely, restore clean stock images from the exact
firmware and slot:

| Installed flow | Images to restore |
| --- | --- |
| GKI 1.0 file patch | Stock `boot.img` |
| GKI 2.0 file patch | Stock `init_boot.img` |
| GKI 2.0 Direct Install / inactive slot | Matching stock `init_boot.img` and `boot.img` |

Confirm that the device boots successfully from the restored images before
uninstalling the Manager.

## Project layout

| Path | Purpose |
| --- | --- |
| [`app/`](app/) | Android Manager, JNI bridge, image-patching UI, superuser policy, and module UI. |
| [`kmod/`](kmod/) | `ethereal.ko` source, locked GKI build inputs, prebuilt modules, and verification scripts. |
| [`ethinit/`](ethinit/) | Freestanding early-boot trampoline that selects and loads the matching KO. |
| [`ethd/`](ethd/) | Rust userspace daemon, image patch commands, module runtime, resetprop, and SELinux policy integration. |
| [`ethsu/`](ethsu/) | Small static SuperCall client staged in the ramdisk. |
| [`ramtool/`](ramtool/) | Boot-image, ramdisk, CPIO, compression, and ELF patching library/tool. |
| [`qemu/`](qemu/) | Official-GKI QEMU test harness. |
| [`tests/`](tests/) | Boot-patch, KMI-selection, branding, and release-artifact checks. |

## Building

A complete Git checkout is required because the build derives its version from
Git history. The canonical environment is documented in
[`.github/workflows/build.yml`](.github/workflows/build.yml): JDK 21, Android
SDK platform 37, Build Tools 36.1.0, NDK 29.0.14206865, CMake 3.31.6, Rust
1.98.0 with the `aarch64-linux-android` target, and `cargo-ndk` 4.1.2.

```sh
git clone https://github.com/TongSaiMiao/Ethereal.git
cd Ethereal
./gradlew --no-configuration-cache testDebugUnitTest lintDebug assembleDebug
```

The debug APK is written to `app/build/outputs/apk/debug/`. The Gradle build
also builds `ethd`, `ramtool`, `ethereal-init`, and `ethsu`, then validates
and packages the committed prebuilt KOs. A locally assembled release APK is not
an official signed Ethereal release.

Rebuilding the kernel modules is a separate Linux/WSL workflow with locked GKI
sources and toolchains:

```sh
bash kmod/build-gki.sh android14-6.1
bash kmod/verify-prebuilt.sh
```

See [`kmod/README.md`](kmod/README.md) for the toolchain and provenance model.

### QEMU validation

```sh
bash qemu/build-and-run.sh android14-6.1
bash qemu/run-all.sh
```

See [`qemu/README.md`](qemu/README.md) for the test contract, matrix, cache
location, and the limits of QEMU coverage.

## Documentation and support

- [FAQ (English)](docs/en/faq.md)
- [常见问题（简体中文）](docs/cn/faq_cn.md)
- [常見問題（繁體中文）](docs/cn_tw/faq_cn_tw.md)
- [Ethereal 模块开发指南](docs/cn/ethereal_module.md)
- [Bug reports](https://github.com/TongSaiMiao/Ethereal/issues/new?template=bug_report.yml)
- [Feature requests](https://github.com/TongSaiMiao/Ethereal/issues/new?template=feature_request.yml)

Before reporting a bug, search existing issues and reproduce it with the latest
applicable build. Attach the archive from **Manager → Settings → Send logs**
and include the device, OS, kernel release, Ethereal version, patch target, and
clear reproduction steps.

## Translations

English, Simplified Chinese, and Traditional Chinese are maintainer-owned
reference locales. Other translations may be LLM-assisted. Keep each
translation PR limited to one locale; changes to the reference locales are
handled by the maintainers.

## Third-party software and acknowledgements

- [Magisk](https://github.com/topjohnwu/Magisk): the policy engine and bundled
  BusyBox binary.
- [KernelSU](https://github.com/tiann/KernelSU): the Manager UI and
  Magisk-style module conventions.
- Thanks to [APatch](https://github.com/bmax121/APatch) for its work in the Android root ecosystem.

Recorded upstream revisions, hashes, source links, and applicable licenses are
listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## License

Except where a file or third-party notice says otherwise, Ethereal userspace
and Manager code is licensed under
[GPL-3.0-only](LICENSE). The Ethereal kernel module is licensed under
GPL-2.0-only. Bundled third-party code keeps its upstream license; see
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
