# 常见问题解答

## Ethereal 是什么？

Ethereal 是面向 ARM64 GKI 1.0 和 GKI 2.0 的内核模块 root 方案。它从启动 ramdisk 加载 `ethereal.ko`，不重写 kernel Image。

## 修补启动镜像时具体改了什么？

- GKI 1.0：把 `ethereal-init`、KO 和其他启动文件加入 `boot.img` 的 ramdisk，并在同一份 `boot.img` 的 cmdline 中加入 `rdinit=/ethereal-init`。
- GKI 2.0 离线修补：只选择一份 `init_boot.img`。补丁把启动文件加入其中，将原厂 `/init` 备份为 `init.ethereal.bak`，再增加一个 `PT_LOAD`，让 ELF 入口先进入 Ethereal 加载器；配套 `boot.img` 及其 cmdline 保持不变。不能把只有内核的 GKI 2.0 `boot.img` 作为单文件目标。Direct Install 仍会把 `init_boot` 和 `boot` 作为一个事务成对修补。

GKI 1.0 和 GKI 2.0 Direct Install 通过 `rdinit` 先运行 `/ethereal-init`。GKI 2.0 单文件离线路径先进入注入原厂 `/init` 的加载器，通过 `finit_module()` 加载精确匹配的 KMI 模块，再跳回原 ELF 入口。补丁不替换原厂文件，取消修补时会从 `init.ethereal.bak` 恢复。

## 为什么不是一份 KO 通用所有内核？

同一主版本的内核也可能使用不同 Android KMI、符号版本和 CRC。Ethereal 为每套支持的 KMI 独立构建 KO，启动时只选择能够明确匹配的模块；无法确认时不会猜测，加载失败也会继续启动原厂系统。

## Ethereal 与 Magisk、KernelSU 的主要区别是什么？

Ethereal 对 GKI 1.0 和 Direct Install 使用 `rdinit` 跳板，对 GKI 2.0 单文件离线修补使用带原文件备份的 ELF 入口 hook，并配合按 KMI 构建的 LKM。它保留原厂 kernel Image，也不替换原厂 `/init` 文件；这与替换 ramdisk init 或把 root 代码直接编入内核源码的方案不同。
