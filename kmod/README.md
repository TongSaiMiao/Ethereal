# ethereal.ko

Loadable kernel module loaded from the ramdisk by `ethereal-init`. The stock kernel
image is not patched.

Build one `.ko` per GKI KMI (struct module / kprobe ABI) in WSL:

```
bash kmod/build-gki.sh
```

Outputs:

```
kmod/prebuilt/android12-5.4/ethereal.ko
kmod/prebuilt/android12-5.10/ethereal.ko
kmod/prebuilt/android13-5.10/ethereal.ko
kmod/prebuilt/android13-5.15/ethereal.ko
kmod/prebuilt/android14-5.15/ethereal.ko
kmod/prebuilt/android14-6.1/ethereal.ko
kmod/prebuilt/android15-6.6/ethereal.ko
kmod/prebuilt/android16-6.12/ethereal.ko
```

`ethd` embeds every validated ELF it finds there. At boot, `ethereal-init`
picks `ethereal.<kmi>.ko` from the running kernel release and loads it with
`finit_module`. Each shipped module must be built after the matching GKI Image
so its `__versions` section contains real symbol CRCs. Ethereal may ignore an
exact patch-level `vermagic` mismatch within the same KMI, but never bypasses
symbol-version checks.

## Locked build toolchains

Every KMI uses the Clang subtree named by its committed official GKI manifest.
`kmod/toolchain-locks.tsv` pins the repository commit, subtree name, and Git
tree for all eight builds; an Android NDK compiler is not a release fallback.
Android 15 / 6.6 and Android 16 / 6.12 additionally use their locked Rust
subtrees when preparing the official kernel configuration.

The build also requires `pahole >= 1.25`. Release verification binds the KO to
the locked compiler and pahole provenance, checks that the canonical and
prepared configurations preserve the official ABI-relevant settings, and
checks the generated `__this_module` layout against the official BTF-enabled
kernel configuration. A KO is rejected if BTF, extended modversions, compiler
identity, symbol CRCs, or module layout do not match its KMI.
