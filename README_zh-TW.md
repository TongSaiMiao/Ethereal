<div align="center">
  <p>
    <a href="README.md">English</a> ·
    <a href="README_zh-CN.md">简体中文</a> ·
    <strong>繁體中文</strong>
  </p>
  <img
    src="app/src/main/res/drawable-nodpi/ic_launcher_monochrome.png"
    alt="Ethereal 應用程式圖示"
    width="120"
  />
  <h1>Ethereal</h1>
  <p><strong>透過 ramdisk 部署且與 KMI 精確相符的核心模組 Root 方案，適用於 ARM64 Android GKI 裝置。</strong></p>
  <p>
    <a href="https://github.com/TongSaiMiao/Ethereal/actions/workflows/build.yml">
      <img
        src="https://github.com/TongSaiMiao/Ethereal/actions/workflows/build.yml/badge.svg?branch=main"
        alt="建置管理器"
      />
    </a>
    <img
      src="https://img.shields.io/badge/platform-Android%20ARM64-3DDC84?logo=android&logoColor=white"
      alt="Android ARM64"
    />
  </p>
  <p>
    <a href="#相容性">相容性</a> ·
    <a href="#安裝">安裝</a> ·
    <a href="#建置">建置</a> ·
    <a href="#文件與支援">文件</a> ·
    <a href="#授權條款">授權條款</a>
  </p>
</div>

Ethereal 是一套 Root 實作，適用於執行受支援 GKI 1.0 或 GKI 2.0 核心的
Android 裝置。它會將每個 Android 核心模組介面 (KMI) 各自對應的模組放入
通用 ramdisk，在早期開機階段載入完全相符的 `ethereal.<kmi>.ko`，然後將
控制權交還 Android。

核心 `Image` 本體絕不會被重寫。GKI 1.0 檔案修補會在單一 `boot.img`
加入 `rdinit=/ethereal-init` 跳板，且不變更 `/init`。GKI 2.0 檔案修補
只需一個 `init_boot.img`，它會將原始 `/init` 保留為
`init.ethereal.bak`，並透過新增的載入器 `PT_LOAD` 重新導向其 ELF 進入點；
`boot.img` 及其命令列均保持不變。

> [!CAUTION]
> 刷寫攸關開機的映像檔可能導致裝置無法開機，且在復原過程中可能造成資料
> 遺失。請備份資料，以及目前韌體和槽位所對應的原廠映像檔。除非您已有經過
> 測試、可返回 bootloader、fastboot 或裝置 OEM 復原工具的途徑，否則請勿繼續。

## 特色

- **保留核心本體。** Ethereal 會修改開機中繼資料與通用 ramdisk；不會重寫
  核心 `Image` 二進位檔。
- **保留原始 init。** GKI 1.0 不會變更 `/init`。GKI 2.0 會保留完全相同的
  備份、執行 `ethereal-init`，再進入原始的第一階段 init 程式碼。
- **精確比對 Android KMI。** 每個受支援的 KMI 都有專屬 KO，並帶有符號版本
  CRC。對於未知或無法明確判定的 KMI，絕不會套用通用備援模組。
- **失敗時仍回到原廠開機路徑。** 若無法選取或載入相符的 KO，Ethereal 會
  略過該模組，並繼續執行原廠 `/init`。
- **驗證管理器身分。** 管理器專用操作同時需要管理器的 Android UID，以及
  首次啟動時產生的 32 位元組私密權杖。
- **無 Root 也能使用管理器。** 管理器可以顯示安裝狀態，並修補使用者選取的
  映像檔，而不需要具特權的分割區存取權。
- **內含使用者空間管理功能。** 管理器提供個別應用程式的超級使用者策略，
  以及由 `ethd` 支援、類似 Magisk 的模組生命週期。

Android 管理器套件 ID：`me.ethereal.app`

## 運作方式

原廠核心必須允許載入模組（`CONFIG_MODULES`）。GKI 1.0 會在單一已修補的
`boot.img` 中使用 `rdinit=/ethereal-init` 跳板。離線 GKI 2.0 修補只接收
一個 `init_boot.img`，將載入器注入原廠 `/init`，並保持 `boot.img` 及其
cmdline 不變；僅含核心的 GKI 2.0 `boot.img` 並非獨立修補目標。修補並安裝
仍會將相符的 `init_boot` 與 `boot` 分割區視為同一交易更新。

```text
Bootloader
└── 原廠核心 Image
    ├── GKI 1.0 / 修補並安裝：rdinit=/ethereal-init
    │   ├── 辨識執行中的 Android KMI
    │   ├── 使用 finit_module() 載入 ethereal.<kmi>.ko
    │   └── 執行原廠 /init
    └── GKI 2.0 檔案修補：掛鉤 /init ELF 進入點
        ├── 執行 ethereal-init 並載入完全相符的 KMI 模組
        └── 進入原始的第一階段 init
            └── Android → Ethereal 管理器 ↔ 經過驗證的 SuperCall
                                      └── ethd 模組服務
```

### 開機映像檔變更

| 流程 | 修補的映像檔 | 變更 |
| --- | --- | --- |
| GKI 1.0 檔案修補 | 單一 `boot.img` | 將載入器、KMI 模組、驗證資訊與私有 `su` 承載內容加入 ramdisk；並將 `rdinit=/ethereal-init` 加入此映像檔的開機命令列。 |
| GKI 2.0 檔案修補 | 單一 `init_boot.img` | 加入相同承載內容，備份實際根目錄中的 `/init`，並掛鉤其 ELF 進入點；不會讀取或修改 `boot.img`。 |
| GKI 2.0 修補並安裝／非活動槽位 | 相符的 `init_boot` + `boot` 分割區 | 將承載內容加入 `init_boot`，並將 `rdinit=/ethereal-init` 加入 `boot`；兩個輸出會以同一交易暫存並發布。 |

檔案選擇器只接受一個映像檔，並在解包後偵測其配置。僅含核心的 `boot.img`、
`vendor_boot.img`、無法明確判定的映像檔，或已被其他方案修補的映像檔都會
遭到拒絕，而不會猜測處理。AOSP 將 `init_boot` 說明為出廠搭載 Android 13
之裝置的分割區配置變更；出廠時搭載較舊版本、之後升級的裝置仍可能將通用
ramdisk 保留在 `boot` 中。請參閱
[AOSP 的通用開機分割區文件](https://source.android.com/docs/core/architecture/partitions/generic-boot)。

## 相容性

Ethereal 目前隨附以下 ARM64 模組：

| GKI 世代 | Android KMI | 核心 |
| --- | --- | --- |
| GKI 1.0 | `android12-5.4` | 5.4 |
| GKI 2.0 | `android12-5.10` | 5.10 |
| GKI 2.0 | `android13-5.10` | 5.10 |
| GKI 2.0 | `android13-5.15` | 5.15 |
| GKI 2.0 | `android14-5.15` | 5.15 |
| GKI 2.0 | `android14-6.1` | 6.1 |
| GKI 2.0 | `android15-6.6` | 6.6 |
| GKI 2.0 | `android16-6.12` | 6.12 |

KMI 名稱中的 Android 版本代表核心分支，不一定是裝置目前執行的 Android
版本。只有核心主版本相符並不足夠。Ethereal 會從核心 release 辨識 KMI，
並且仍會強制核對模組符號版本 CRC。

表格中的每一列都有已提交的預先建置 KO，並以完全一致的 Google GKI 映像檔
進行 QEMU 測試。這可驗證發行模組適用於該 GKI 建置版本；但這**不**表示它
一定與衍生自相同 KMI 的每個 OEM 供應商核心相容。

管理器 APK 需要 Android 8.0 或更新版本（API 26）及 ARM64；隨附的 Root
承載內容則僅支援上列 KMI。

### 需求

- 具備受支援、且可明確辨識之 Android KMI 的 ARM64 裝置。
- 以 `CONFIG_MODULES=y` 建置，且允許載入外部模組的原廠核心。
- 與目前韌體及槽位完全相符的乾淨原廠開機映像檔。
- 可刷寫及還原開機分割區的方法，通常是已解鎖的 bootloader，或已可正常
  運作的特權安裝途徑。
- 在修補或刷寫之前，完整備份重要資料。

### 已知限制

- OEM 核心變更、模組簽章策略、CFI 或額外的平台防護，仍可能拒絕可在相應
  Google GKI 映像檔上運作的 KO。
- 具有額外 OEM 核心防護的 Samsung 裝置目前尚未測試，也不受支援。
- 偵測到 Magisk 時，Ethereal 的模組開機路徑會被略過。系統覆蓋層也需要
  合適的 OverlayFS／元模組設定。
- Ethereal 不包含 Zygisk。Ethereal 模組 ZIP 必須透過管理器安裝，不支援
  在 Recovery 中安裝。

## 安裝

經簽署並加上標籤的建置版本會發布於
[GitHub Releases](https://github.com/TongSaiMiao/Ethereal/releases)。若沒有可用的
標籤版本，請從原始碼建置管理器。未加標籤的 Actions 建置產物屬於開發版本，
不應視為經簽署的發行版本。

> [!IMPORTANT]
> 修補前請至少開啟管理器一次。管理器會產生私密驗證權杖，並將該權杖連同
> 管理器 UID 嵌入每個已修補的 ramdisk。切勿發布或分享已修補的映像檔。
> 刷寫後，請勿清除管理器的應用程式資料、解除安裝後重新安裝，或改用不同
> 簽章的建置版本；否則管理器存取權可能失效，您必須使用乾淨的原廠映像檔
> 重新修補。一般的覆蓋更新會保留應用程式資料。

### 首次安裝

1. 安裝並開啟 Ethereal 管理器。
2. 備份目前韌體與槽位的乾淨原廠映像檔。請勿使用已由 Ethereal、Magisk 或
   其他修補工具修改過的映像檔。
3. 開啟**首頁 → 安裝**，並選擇以檔案為基礎的修補選項。此流程不需要 Root，
   也不會觸及實體分割區。
4. 僅選取一個原廠映像檔：通用 ramdisk 位於 boot 時選取 `boot.img`；
   裝置具有獨立 init_boot 分割區時選取 `init_boot.img`。
5. 檢查修補日誌。一個 `Ethereal-<original-filename>` 輸出檔會寫入
   `Downloads`。
6. 將輸出檔刷回相同類型的分割區。分割區名稱與 A/B 槽位處理方式因 OEM
   而異；切勿將 `init_boot` 映像檔刷入 `boot`。
7. 重新啟動並開啟**原本的管理器應用程式（不要解除安裝後重裝）**。
   核心模組與管理器驗證資訊通過檢查後，管理器會部署 `ethd` 使用者空間
   服務，並啟用適用的超級使用者與模組分頁。

**修補並安裝**和**安裝到非活動槽位**需要一套已正常運作且通過驗證的 Ethereal
安裝。這些選項適用於更新、修復及 OTA 後的處理，而非首次安裝。在 GKI 2.0
上，這些特權流程會一併修補相符的 `init_boot` 與 `boot` 分割區。只有在
OTA 完成後，才能使用非活動槽位選項。

### 移除與復原

管理器的**解除安裝**操作只會移除使用者空間模組服務，不會從已刷寫的映像檔
移除 `rdinit` 或 `ethereal.ko`。

若要完整移除 Ethereal，請還原與目前韌體及槽位完全相符的乾淨原廠映像檔：

| 已安裝的流程 | 要還原的映像檔 |
| --- | --- |
| GKI 1.0 檔案修補 | 原廠 `boot.img` |
| GKI 2.0 檔案修補 | 原廠 `init_boot.img` |
| GKI 2.0 修補並安裝／非活動槽位 | 相符的原廠 `init_boot.img` 與 `boot.img` |

解除安裝管理器前，請確認裝置可從還原後的映像檔成功開機。

## 專案結構

| 路徑 | 用途 |
| --- | --- |
| [`app/`](app/) | Android 管理器、JNI 橋接、映像檔修補 UI、超級使用者策略與模組 UI。 |
| [`kmod/`](kmod/) | `ethereal.ko` 原始碼、鎖定的 GKI 建置輸入項、預先建置的模組與驗證指令碼。 |
| [`ethinit/`](ethinit/) | 可獨立執行的早期開機跳板，用來選取並載入相符的 KO。 |
| [`ethd/`](ethd/) | Rust 使用者空間常駐程式、映像檔修補命令、模組執行階段、resetprop 與 SELinux 策略整合。 |
| [`ethsu/`](ethsu/) | 置入 ramdisk 的小型靜態 SuperCall 用戶端。 |
| [`ramtool/`](ramtool/) | 開機映像檔、ramdisk、CPIO、壓縮及 ELF 修補程式庫／工具。 |
| [`qemu/`](qemu/) | 官方 GKI QEMU 測試框架。 |
| [`tests/`](tests/) | 開機修補、KMI 選取、品牌識別與發行產物檢查。 |

## 建置

建置程序會從 Git 歷史記錄衍生版本，因此需要完整的 Git checkout。標準環境
記錄於 [`.github/workflows/build.yml`](.github/workflows/build.yml)：JDK 21、
Android SDK platform 37、Build Tools 36.1.0、NDK 29.0.14206865、CMake
3.31.6、Rust 1.98.0（含 `aarch64-linux-android` 目標），以及 `cargo-ndk`
4.1.2。

```sh
git clone https://github.com/TongSaiMiao/Ethereal.git
cd Ethereal
./gradlew --no-configuration-cache testDebugUnitTest lintDebug assembleDebug
```

偵錯 APK 會寫入 `app/build/outputs/apk/debug/`。Gradle 建置也會建置 `ethd`、
`ramtool`、`ethereal-init` 與 `ethsu`，然後驗證並封裝已提交的預先建置 KO。
在本機組建的 release APK 並不是 Ethereal 官方簽署的發行版本。

重新建置核心模組是另一套 Linux／WSL 工作流程，使用已鎖定的 GKI 原始碼與
工具鏈：

```sh
bash kmod/build-gki.sh android14-6.1
bash kmod/verify-prebuilt.sh
```

工具鏈及來源追溯模型請參閱 [`kmod/README.md`](kmod/README.md)。

### QEMU 驗證

```sh
bash qemu/build-and-run.sh android14-6.1
bash qemu/run-all.sh
```

測試契約、矩陣、快取位置與 QEMU 測試範圍限制，請參閱
[`qemu/README.md`](qemu/README.md)。

## 文件與支援

- [常見問題（英文）](docs/en/faq.md)
- [常見問題（簡體中文）](docs/cn/faq_cn.md)
- [常見問題（繁體中文）](docs/cn_tw/faq_cn_tw.md)
- [Ethereal 模組開發指南（簡體中文）](docs/cn/ethereal_module.md)
- [回報錯誤](https://github.com/TongSaiMiao/Ethereal/issues/new?template=bug_report.yml)
- [提出功能需求](https://github.com/TongSaiMiao/Ethereal/issues/new?template=feature_request.yml)

回報錯誤前，請先搜尋現有 issue，並使用最新的適用建置版本重現問題。請附上
從**管理器 → 設定 → 發送日誌**取得的壓縮檔，並提供裝置、作業系統、核心
release、Ethereal 版本、修補目標及清楚的重現步驟。

## 翻譯

英文、簡體中文和繁體中文是由維護者負責的參考語系。其他翻譯可能由 LLM
協助。每個翻譯 PR 請僅包含一個語系；參考語系的變更由維護者處理。

## 第三方軟體與致謝

- [Magisk](https://github.com/topjohnwu/Magisk)：策略引擎與隨附的 BusyBox
  二進位檔。
- [KernelSU](https://github.com/tiann/KernelSU)：管理器 UI 與類似 Magisk 的
  模組慣例。
- 感謝 [APatch](https://github.com/bmax121/APatch) 對 Android Root 生態系的貢獻。

上游修訂版本、雜湊、原始碼連結與適用授權條款的記錄，均列於
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

## 授權條款

除非個別檔案或第三方聲明另有說明，Ethereal 使用者空間與管理器程式碼均採用
[GPL-3.0-only](LICENSE) 授權。Ethereal 核心模組採用 GPL-2.0-only 授權。
隨附的第三方程式碼保留其上游授權；詳見
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。
