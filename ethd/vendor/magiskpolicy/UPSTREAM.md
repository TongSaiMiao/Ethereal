# Upstream sources

The policy engine in this directory comes directly from Magisk commit
`8903cf7f2261fb60bd2d0b568d6becb2ecf74c3e`:

<https://github.com/topjohnwu/Magisk/tree/8903cf7f2261fb60bd2d0b568d6becb2ecf74c3e/native/src/sepolicy>

The bundled `libsepol` comes from the `topjohnwu/selinux` fork submodule commit
pinned by that Magisk revision, `be1b39a657fee7faacfae548b75cb53302043a01`:

<https://github.com/topjohnwu/selinux/tree/be1b39a657fee7faacfae548b75cb53302043a01/libsepol>

Magisk's Rust parser, built-in rules, and C++ policy implementation are kept
as the upstream implementation. Ethereal adds a standalone Cargo build, a
small standard C ABI, and portable replacements for Magisk's internal base
helpers. `policydb.cpp`, `sepolicy.cpp`, `rules.rs`, and `statement.rs` carry
small marked changes needed to use those adapters outside Magisk's private
workspace.

Magisk's GPL-3.0 license is in `LICENSE`. The SELinux fork sources retain their
license and copyright notices in `libsepol/LICENSE` and the individual source
files.
