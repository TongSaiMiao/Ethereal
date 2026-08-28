<div align="center">
  <p>
    <a href="README.md">English</a> ·
    <strong>简体中文</strong> ·
    <a href="README_zh-TW.md">繁體中文</a>
  </p>
  <img
    src="app/src/main/res/drawable-nodpi/ic_launcher_monochrome.png"
    alt="Ethereal 应用图标"
    width="120"
  />
  <h1>Ethereal</h1>
  <p><strong>面向 ARM64 Android GKI 设备、通过 ramdisk 部署且精确匹配 KMI 的内核模块 root 实现。</strong></p>
  <p>
    <a href="https://github.com/TongSaiMiao/Ethereal/actions/workflows/build.yml">
      <img
        src="https://github.com/TongSaiMiao/Ethereal/actions/workflows/build.yml/badge.svg?branch=main"
        alt="构建管理器"
      />
    </a>
    <img
      src="https://img.shields.io/badge/platform-Android%20ARM64-3DDC84?logo=android&logoColor=white"
      alt="Android ARM64"
    />
  </p>
  <p>
    <a href="#兼容性">兼容性</a> ·
    <a href="#安装">安装</a> ·
    <a href="#构建">构建</a> ·
    <a href="#文档与支持">文档</a> ·
    <a href="#许可证">许可证</a>
  </p>
</div>

Ethereal 是一套 Android root 实现，适用于运行受支持 GKI 1.0 或 GKI 2.0
内核的设备。它会将每个 Android 内核模块接口（KMI）各自对应的模块放入通用
ramdisk，在早期启动阶段加载精确匹配的 `ethereal.<kmi>.ko`，然后将控制权
交还 Android。

内核 `Image` 载荷绝不会被重写。GKI 1.0 文件修补会向单个 `boot.img`
加入 `rdinit=/ethereal-init` 跳板，并保持 `/init` 不变。GKI 2.0 文件修补
只需一个 `init_boot.img`，它会将原始 `/init` 保留为
`init.ethereal.bak`，并通过新增的加载器 `PT_LOAD` 重定向其 ELF 入口；
`boot.img` 及其命令行保持不变。

> [!CAUTION]
> 刷写启动关键镜像可能导致设备无法启动，并且在恢复过程中可能造成数据
> 丢失。请备份数据以及当前固件和槽位对应的原厂镜像。除非已经验证能够
> 返回 Bootloader、fastboot 或设备的 OEM 恢复工具，否则请勿继续。

## 主要特性

- **保留内核载荷。** Ethereal 会修改启动元数据和通用 ramdisk；不会重写
  内核 `Image` 二进制文件。
- **保留原始 init。** GKI 1.0 保持 `/init` 不变。GKI 2.0 会保留精确备份，
  运行 `ethereal-init`，然后进入原始的第一阶段 init 代码。
- **精确匹配 Android KMI。** 每个受支持的 KMI 都有各自包含符号版本 CRC
  的 KO。未知或有歧义的 KMI 绝不会被分配通用的回退模块。
- **失败时回退至原厂启动路径。** 如果无法选择或加载匹配的 KO，Ethereal
  会跳过该模块并继续执行原厂 `/init`。
- **验证管理器身份。** 仅限管理器的操作同时需要管理器的 Android UID 和
  首次启动时生成的私有 32 字节身份验证令牌（token）。
- **无需 root 也可使用管理器。** 管理器无需特权分区访问即可显示安装状态，
  并修补用户选择的镜像文件。
- **包含用户空间管理功能。** 管理器提供逐应用超级用户策略，以及由 `ethd`
  支撑、采用 Magisk 风格的模块生命周期。

Android 管理器软件包 ID：`me.ethereal.app`

## 工作原理

原厂内核必须允许加载模块（`CONFIG_MODULES`）。GKI 1.0 在单个已修补的
`boot.img` 中使用 `rdinit=/ethereal-init` 跳板。离线 GKI 2.0 修补接收
单个 `init_boot.img`，将加载器注入原厂 `/init`，并保持 `boot.img` 及其
cmdline 不变；仅包含内核的 GKI 2.0 `boot.img` 不是独立修补目标。直接安装
仍会将匹配的 `init_boot` 和 `boot` 分区作为同一事务更新。

```text
Bootloader
└── 原厂内核 Image
    ├── GKI 1.0 / 直接安装：rdinit=/ethereal-init
    │   ├── 识别正在运行的 Android KMI
    │   ├── 使用 finit_module() 加载 ethereal.<kmi>.ko
    │   └── 执行原厂 /init
    └── GKI 2.0 文件修补：挂钩 /init ELF 入口
        ├── 运行 ethereal-init 并加载精确匹配的 KMI 模块
        └── 进入原始第一阶段 init
            └── Android → Ethereal 管理器 ↔ 经身份验证的 SuperCall
                                      └── ethd 模块服务
```

### 启动镜像变更

| 流程 | 修补的镜像 | 变更 |
| --- | --- | --- |
| GKI 1.0 文件修补 | 单个 `boot.img` | 将加载器、KMI 模块、身份凭据和私有 `su` 载荷加入 ramdisk；将 `rdinit=/ethereal-init` 加入该镜像的启动命令行。 |
| GKI 2.0 文件修补 | 单个 `init_boot.img` | 加入相同载荷，备份实际根目录中的 `/init`，并挂钩其 ELF 入口；不会读取或修改 `boot.img`。 |
| GKI 2.0 直接安装／未使用槽位 | 匹配的 `init_boot` + `boot` 分区 | 将载荷加入 `init_boot`，并将 `rdinit=/ethereal-init` 加入 `boot`；两个输出会作为同一事务暂存并发布。 |

文件选择器只接收一个镜像，并在解包后检测其布局。仅包含内核的 `boot.img`、
`vendor_boot.img`、有歧义的镜像或已被其他方案修补的镜像都会被拒绝，而不会
被猜测处理。AOSP 文档将 `init_boot` 说明为首发搭载 Android 13 的设备所采用
的分区布局变更；首发搭载较早版本、之后升级的设备仍可将通用 ramdisk 保留在
`boot` 中。参见
[AOSP 通用启动分区文档](https://source.android.com/docs/core/architecture/partitions/generic-boot)。

## 兼容性

Ethereal 当前包含以下 ARM64 模块：

| GKI 代际 | Android KMI | 内核 |
| --- | --- | --- |
| GKI 1.0 | `android12-5.4` | 5.4 |
| GKI 2.0 | `android12-5.10` | 5.10 |
| GKI 2.0 | `android13-5.10` | 5.10 |
| GKI 2.0 | `android13-5.15` | 5.15 |
| GKI 2.0 | `android14-5.15` | 5.15 |
| GKI 2.0 | `android14-6.1` | 6.1 |
| GKI 2.0 | `android15-6.6` | 6.6 |
| GKI 2.0 | `android16-6.12` | 6.12 |

KMI 名称中的 Android 版本标识的是内核分支，不一定是设备当前运行的
Android 版本。仅主内核版本匹配并不足够。Ethereal 会根据内核 release
识别 KMI，并且仍会强制校验模块符号版本 CRC。

表中的每一项都有已提交的预构建 KO，并使用精确的 Google GKI 镜像进行
QEMU 覆盖测试。这可以验证发布模块与该 GKI 构建的兼容性；但**不能**证明
它兼容从同一 KMI 派生的每一个 OEM 厂商内核。

管理器 APK 要求 Android 8.0 或更高版本（API 26）和 ARM64，而随附的 root
载荷仅限于上面列出的 KMI。

### 要求

- 一台采用 ARM64、其 Android KMI 受支持且可被无歧义识别的设备。
- 使用 `CONFIG_MODULES=y` 构建并允许加载外部模块的原厂内核。
- 来自当前固件和槽位的干净原厂启动镜像。
- 一种可以刷写和恢复启动分区的方式，通常是已解锁的 Bootloader，或已经
  可用的特权安装路径。
- 在修补或刷写前完整备份重要数据。

### 已知限制

- OEM 内核变更、模块签名策略、CFI 或额外的平台保护仍可能拒绝能在对应
  Google GKI 镜像上工作的 KO。
- 带有额外 OEM 内核保护的三星设备目前未经测试，也不受支持。
- 检测到 Magisk 时会跳过 Ethereal 的模块启动路径。系统 overlay 也需要
  合适的 OverlayFS/元模块配置。
- Ethereal 不包含 Zygisk。Ethereal 模块 ZIP 必须从管理器安装，不支持在
  Recovery 中安装。

## 安装

带签名的正式版本会通过
[GitHub Releases](https://github.com/TongSaiMiao/Ethereal/releases) 发布。如果
没有带标签的版本，请从源码构建管理器。非标签版本的 Actions 产物属于
开发版本，不应视为带签名的正式版本。

> [!IMPORTANT]
> 请在修补前**至少打开管理器一次**。管理器会创建一个私有身份验证令牌，
> 并将该令牌与管理器 UID 一同嵌入每个修补后的 ramdisk。切勿发布或分享
> 修补后的镜像。刷写后，请勿清除管理器的应用数据、卸载后重新安装，或改用
> 签名不同的构建；这些操作可能使管理器访问失效，并要求使用干净原厂镜像
> 重新修补。正常的覆盖安装更新会保留应用数据。

### 首次安装

1. 安装并打开 Ethereal 管理器。
2. 备份当前固件和槽位的干净原厂镜像。请勿使用已经被 Ethereal、Magisk 或
   其他修补工具修改过的镜像。
3. 打开**主页 → 安装**，选择基于文件的修补选项。此流程不需要 root，
   也不会改动任何物理分区。
4. 只选择一个原厂镜像：通用 ramdisk 位于 boot 时选择 `boot.img`；设备有
   独立 init_boot 分区时选择 `init_boot.img`。
5. 检查修补日志。一个 `Ethereal-<original-filename>` 输出会写入
   `Downloads`。
6. 将输出刷回同一类型的分区。分区名称和 A/B 槽位处理方式因 OEM 而异；
   绝不要将 `init_boot` 镜像刷入 `boot`。
7. 重启并打开**原管理器应用（不要卸载重装）**。验证内核模块和管理器
   凭据后，管理器会部署 `ethd` 用户空间服务，并启用适用的超级用户和系统模块标签页。

**直接安装**和**安装到未使用的槽位**需要已经可用且通过身份验证的 Ethereal
安装。它们用于更新、修复和 OTA 后的处理，而不是首次安装。在 GKI 2.0 上，
这些特权流程会同时修补匹配的 `init_boot` 和 `boot` 分区。仅可在 OTA
完成后使用未使用槽位选项。

### 移除与恢复

管理器的**卸载**操作只会移除用户空间模块服务。它不会从已刷写的镜像中移除
`rdinit` 或 `ethereal.ko`。

要彻底移除 Ethereal，请恢复与当前固件和槽位完全匹配的干净原厂镜像：

| 已安装的流程 | 要恢复的镜像 |
| --- | --- |
| GKI 1.0 文件修补 | 原厂 `boot.img` |
| GKI 2.0 文件修补 | 原厂 `init_boot.img` |
| GKI 2.0 直接安装／未使用槽位 | 匹配的原厂 `init_boot.img` 和 `boot.img` |

请先确认设备能够从恢复后的镜像成功启动，再卸载管理器。

## 项目结构

| 路径 | 用途 |
| --- | --- |
| [`app/`](app/) | Android 管理器、JNI 桥接、镜像修补界面、超级用户策略和模块界面。 |
| [`kmod/`](kmod/) | `ethereal.ko` 源码、锁定的 GKI 构建输入、预构建模块和验证脚本。 |
| [`ethinit/`](ethinit/) | 在早期启动阶段选择并加载匹配 KO 的独立运行跳板。 |
| [`ethd/`](ethd/) | Rust 用户空间守护进程、镜像修补命令、模块运行时、resetprop 和 SELinux 策略集成。 |
| [`ethsu/`](ethsu/) | 置于 ramdisk 中的小型静态 SuperCall 客户端。 |
| [`ramtool/`](ramtool/) | 启动镜像、ramdisk、CPIO、压缩和 ELF 修补库/工具。 |
| [`qemu/`](qemu/) | 官方 GKI QEMU 测试框架。 |
| [`tests/`](tests/) | 启动修补、KMI 选择、品牌标识和发布产物检查。 |

## 构建

构建过程会从 Git 历史记录中推导版本，因此需要完整的 Git checkout。标准环境
记录在 [`.github/workflows/build.yml`](.github/workflows/build.yml) 中：JDK 21、
Android SDK platform 37、Build Tools 36.1.0、NDK 29.0.14206865、CMake 3.31.6、
Rust 1.98.0（包含 `aarch64-linux-android` target）以及 `cargo-ndk` 4.1.2。

```sh
git clone https://github.com/TongSaiMiao/Ethereal.git
cd Ethereal
./gradlew --no-configuration-cache testDebugUnitTest lintDebug assembleDebug
```

调试版 APK 会写入 `app/build/outputs/apk/debug/`。Gradle 构建还会构建
`ethd`、`ramtool`、`ethereal-init` 和 `ethsu`，随后验证并打包已提交的
预构建 KO。本地组装的 release APK 并非 Ethereal 官方签名版本。

重新构建内核模块是一套独立的 Linux/WSL 流程，会使用锁定的 GKI 源码和
工具链：

```sh
bash kmod/build-gki.sh android14-6.1
bash kmod/verify-prebuilt.sh
```

有关工具链和来源追溯模型，请参阅 [`kmod/README.md`](kmod/README.md)。

### QEMU 验证

```sh
bash qemu/build-and-run.sh android14-6.1
bash qemu/run-all.sh
```

有关测试约定、矩阵、缓存位置及 QEMU 覆盖范围的限制，请参阅
[`qemu/README.md`](qemu/README.md)。

## 文档与支持

- [常见问题（英文）](docs/en/faq.md)
- [常见问题（简体中文）](docs/cn/faq_cn.md)
- [常见问题（繁体中文）](docs/cn_tw/faq_cn_tw.md)
- [Ethereal 模块开发指南](docs/cn/ethereal_module.md)
- [报告错误](https://github.com/TongSaiMiao/Ethereal/issues/new?template=bug_report.yml)
- [功能请求](https://github.com/TongSaiMiao/Ethereal/issues/new?template=feature_request.yml)

报告错误前，请先搜索现有 issue，并使用最新适用版本复现问题。附上通过
**管理器 → 设置 → 发送日志**生成的归档文件，并提供设备、操作系统、内核
release、Ethereal 版本、修补目标及清晰的复现步骤。

## 翻译

英文、简体中文和繁体中文是由维护者负责的参考语言。其他翻译可能借助 LLM
完成。请将每个翻译 PR 限定为一种语言；参考语言的变更由维护者处理。

## 第三方软件与致谢

- [Magisk](https://github.com/topjohnwu/Magisk)：策略引擎和随附的 BusyBox
  二进制文件。
- [KernelSU](https://github.com/tiann/KernelSU)：管理器界面和 Magisk 风格的
  模块约定。
- 感谢 [APatch](https://github.com/bmax121/APatch) 对 Android root 生态的贡献。

已记录的上游修订版本、哈希值、源码链接和适用许可证均列于
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## 许可证

除非文件或第三方声明另有说明，Ethereal 用户空间和管理器代码均采用
[GPL-3.0-only](LICENSE) 许可证。Ethereal 内核模块采用 GPL-2.0-only
许可证。随附的第三方代码保留其上游许可证；参见
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。
