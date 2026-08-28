# 常见问题解答

## Ethereal 是什么？

Ethereal 是面向 ARM64 GKI 1.0 和 GKI 2.0 的内核模块 root 方案。它从启动 ramdisk 加载 `ethereal.ko`，不重写 kernel Image。

## 修补启动镜像时具体改了什么？

- GKI 1.0：把 `ethereal-init`、KO 和其他启动文件加入 `boot.img` 的 ramdisk，并在同一份 `boot.img` 的 cmdline 中加入 `rdinit=/ethereal-init`。
- GKI 2.0：把这些文件加入 `init_boot.img` 的 ramdisk，并在配套 `boot.img` 的 cmdline 中加入 `rdinit=/ethereal-init`。因此必须成对修补 `init_boot.img` 和 `boot.img`。

内核先运行 `/ethereal-init`。它按当前内核 release 选择精确匹配的 KMI 模块，通过 `finit_module()` 加载，然后执行原厂 `/init` 继续启动。Ethereal 不替换原厂 `/init`，也不修改它的 ELF 入口。

## 为什么不是一份 KO 通用所有内核？

同一主版本的内核也可能使用不同 Android KMI、符号版本和 CRC。Ethereal 为每套支持的 KMI 独立构建 KO，启动时只选择能够明确匹配的模块；无法确认时不会猜测，加载失败也会继续启动原厂系统。

## Ethereal 与 Magisk、KernelSU 的主要区别是什么？

Ethereal 的核心路径是 `rdinit` 跳板和按 KMI 构建的 LKM。它保留原厂 kernel Image 和 `/init` 文件；这与替换 ramdisk init 的方案，以及把 root 代码直接编入内核源码的方案都不同。
