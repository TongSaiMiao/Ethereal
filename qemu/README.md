# QEMU official-GKI Ethereal harness

Physical partitions are never touched. Each test boots the exact Google GKI
Image identified by the committed build manifest and loads the unchanged
release `ethereal.ko` from a tiny initramfs. The harness does not rebuild or
rewrite the kernel Image, so the module loader, `vermagic`, symbol CRCs, CFI,
and `struct module` layout are the ones shipped by Google.

The pid-1 test program loads `ethereal.ko` with `finit_module()` and checks:

1. a hidden 32-byte manager token and manager UID are both required
2. a reused manager UID with the wrong token is rejected
3. transferring an authenticated fd does not bypass caller checks
4. the manager can use HELLO, storage operations, and SU
5. an allowlisted UID can use SU but cannot use manager-only operations

Pass/fail is `ETHEREAL_QEMU_RESULT=PASS` on the serial console. A QEMU pass
proves that the release KO loads and runs on that exact official GKI build. It
does not model an OEM vendor kernel, bootloader flashing, AVB policy, or device
hardware; those remain device-specific checks.

## Matrix

| KMI | GKI | Exact official Image + release KO |
|---|---|---|
| android12-5.4 | 1.0 | PASS |
| android12-5.10 | 1.0 | PASS |
| android13-5.10 | 1.0/2.0 | PASS |
| android13-5.15 | 2.0 | PASS |
| android14-5.15 | 2.0 | PASS |
| android14-6.1 | 2.0 | PASS |
| android15-6.6 | 2.0 | PASS |
| android16-6.12 | 2.0 | PASS |

## Commands

```sh
bash kmod/build-gki.sh android14-6.1
bash qemu/build-and-run.sh android14-6.1
bash qemu/run-all.sh
bash tests/run-gki1-boot-patch-e2e.sh
bash tests/run-boot-patch-e2e.sh
```

The first patch E2E command exercises an offline, unprivileged GKI 1.0
single-`boot.img` patch and QEMU handoff. The second exercises both GKI 2.0
paths: an offline single-`init_boot.img` ELF-entry hook with the stock
`boot.img` cmdline unchanged, and the paired `init_boot`/`boot` transaction
used by Direct Install. Both GKI 2.0 handoff paths are booted in QEMU.

The official Images are cached under `/root/gki-official/<kmi>/`. Serial logs
are written to `qemu/out/<kmi>/serial.log`; `qemu/out/matrix.log` contains the
full matrix summary.
