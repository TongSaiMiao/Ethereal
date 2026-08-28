# FAQ

## What is Ethereal?

Ethereal is a kernel-module root solution for ARM64 GKI 1.0 and GKI 2.0. It loads `ethereal.ko` from the boot ramdisk without rewriting the kernel Image.

## What does the boot-image patch change?

- GKI 1.0: `ethereal-init`, the KOs, and the other boot payload are added to the `boot.img` ramdisk. `rdinit=/ethereal-init` is added to that same `boot.img` cmdline.
- GKI 2.0 offline patch: select one `init_boot.img`. The payload is added there, the original `/init` is saved as `init.ethereal.bak`, and an extra `PT_LOAD` redirects its ELF entry through the Ethereal loader. The matching `boot.img` and its cmdline are unchanged. A kernel-only GKI 2.0 `boot.img` is rejected as a standalone target. Direct Install still patches `init_boot` and `boot` together as one transaction.

GKI 1.0 and GKI 2.0 Direct Install start `/ethereal-init` through `rdinit`. The offline GKI 2.0 path enters the loader injected into the stock `/init`, loads the exact KMI module with `finit_module()`, and then jumps to the original ELF entry. The stock file is not replaced, and unpatch restores it from `init.ethereal.bak`.

## Why is there no single universal KO?

Kernels with the same major version can still use different Android KMIs, symbol versions, and CRCs. Ethereal builds one KO for each supported KMI and only loads an unambiguous match. If no exact match can be selected, boot continues without loading Ethereal.

## How does Ethereal differ from Magisk and KernelSU?

Ethereal uses an `rdinit` trampoline for GKI 1.0 and Direct Install, or a backed-up ELF-entry hook for an offline GKI 2.0 `init_boot` patch, plus per-KMI LKMs. It preserves the stock kernel Image and does not replace the stock `/init` file, unlike designs that replace ramdisk init or compile root code directly into a kernel source tree.
