<div align="center">
<h1 align="center">Ethereal</h1>
</div>

Ramdisk SuperCall root for GKI 1.0 and GKI 2.0.

- Does **not** rewrite the kernel Image.
- Does **not** replace ramdisk `/init`; a single-file GKI 2.0 patch keeps the
  original as `init.ethereal.bak` and redirects its ELF entry through an added
  loader `PT_LOAD`.
- Loads `ethereal.ko` from ramdisk, then starts the SuperCall-backed `ethd` daemon.
- Uses an automatically generated Ethereal manager credential; users do not configure a kernel key or kernel-plugin compatibility layer.
- Manager package: `me.ethereal.app`.

## Supported Versions

- Only supports the ARM64 architecture.
- Ships modules for GKI 5.4, 5.10, 5.15, 6.1, 6.6, and 6.12.

Support for Samsung devices with security protection: Planned

## Requirement

The stock kernel must allow loading modules (`CONFIG_MODULES`). GKI 1.0 uses
the `rdinit=/ethereal-init` trampoline in one patched `boot.img`. Offline GKI
2.0 patching accepts one `init_boot.img`, injects the loader into the stock
`/init`, and leaves `boot.img` and its cmdline unchanged; a kernel-only GKI 2.0
`boot.img` is not a standalone patch target. Direct Install still updates the
matching `init_boot` and `boot` partitions as one transaction.

## Translation

Translations are managed by LLM. Chinese and English are the reference languages and do not accept PR corrections. If you want to contribute a new language or improve an existing translation, please open a PR with the specific language only.

## Get Help

### Usage

See [docs/](docs/).

## Source Notices

- [Magisk](https://github.com/topjohnwu/Magisk): magiskpolicy and the bundled
  BusyBox binary. Exact revisions, hashes, corresponding source, and licenses
  are recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- [KernelSU](https://github.com/tiann/KernelSU): App UI, and Magisk module like support.

## Acknowledgement

- Thanks to [APatch](https://github.com/bmax121/APatch) for its work in the Android root ecosystem.

## License

Except where a file or third-party notice says otherwise, Ethereal userspace
and Manager code is licensed under the GNU General Public License v3
[GPL-3](https://www.gnu.org/licenses/gpl-3.0.html). The Ethereal kernel module
is licensed under GPL-2.0-only. Bundled third-party code keeps its upstream
license; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
