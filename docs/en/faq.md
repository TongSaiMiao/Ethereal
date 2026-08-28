# FAQ

## What is Ethereal?

Ethereal is a kernel-module root solution for ARM64 GKI 1.0 and GKI 2.0. It loads `ethereal.ko` from the boot ramdisk without rewriting the kernel Image.

## What does the boot-image patch change?

- GKI 1.0: `ethereal-init`, the KOs, and the other boot payload are added to the `boot.img` ramdisk. `rdinit=/ethereal-init` is added to that same `boot.img` cmdline.
- GKI 2.0: the payload is added to the `init_boot.img` ramdisk, while `rdinit=/ethereal-init` is added to the matching `boot.img` cmdline. The two images must therefore be patched as a pair.

The kernel starts `/ethereal-init`. It selects an exact KMI module from the running kernel release, loads it with `finit_module()`, and then executes the stock `/init`. Ethereal neither replaces the stock `/init` nor changes its ELF entry point.

## Why is there no single universal KO?

Kernels with the same major version can still use different Android KMIs, symbol versions, and CRCs. Ethereal builds one KO for each supported KMI and only loads an unambiguous match. If no exact match can be selected, boot continues without loading Ethereal.

## How does Ethereal differ from Magisk and KernelSU?

Ethereal's core path is an `rdinit` trampoline plus per-KMI LKMs. It preserves the stock kernel Image and `/init` file, unlike designs that replace the ramdisk init or compile root code directly into a kernel source tree.
