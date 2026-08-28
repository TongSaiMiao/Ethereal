# 常見問題集

## 什麼是 Ethereal？

Ethereal 是適用於 ARM64 GKI 1.0 與 GKI 2.0 的核心模組 Root 方案。它從啟動 ramdisk 載入 `ethereal.ko`，不會重寫 kernel Image。

## 修補啟動映像時會更改什麼？

- GKI 1.0：將 `ethereal-init`、KO 與其他啟動檔案加入 `boot.img` 的 ramdisk，並在同一份 `boot.img` 的 cmdline 加入 `rdinit=/ethereal-init`。
- GKI 2.0：將這些檔案加入 `init_boot.img` 的 ramdisk，並在配套 `boot.img` 的 cmdline 加入 `rdinit=/ethereal-init`，因此兩份映像必須成對修補。

核心會先執行 `/ethereal-init`。它依目前核心 release 選擇精確相符的 KMI 模組，以 `finit_module()` 載入後再執行原廠 `/init`。Ethereal 不會取代原廠 `/init`，也不會修改其 ELF 進入點。

## 為什麼不能用同一份 KO 支援所有核心？

相同主版本的核心仍可能採用不同的 Android KMI、符號版本與 CRC。Ethereal 會為每套支援的 KMI 分別建置 KO，啟動時只載入能明確判定的版本；無法精確配對時不會猜測，系統仍會繼續啟動。

## Ethereal 與 Magisk、KernelSU 的主要差異是什麼？

Ethereal 的核心路徑是 `rdinit` 跳板與每 KMI 一份的 LKM。它保留原廠 kernel Image 和 `/init` 檔案，與替換 ramdisk init 或把 Root 程式碼直接編入核心原始碼的方案不同。
