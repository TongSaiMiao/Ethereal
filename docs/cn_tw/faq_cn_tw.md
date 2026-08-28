# 常見問題集

## 什麼是 Ethereal？

Ethereal 是適用於 ARM64 GKI 1.0 與 GKI 2.0 的核心模組 Root 方案。它從啟動 ramdisk 載入 `ethereal.ko`，不會重寫 kernel Image。

## 修補啟動映像時會更改什麼？

- GKI 1.0：將 `ethereal-init`、KO 與其他啟動檔案加入 `boot.img` 的 ramdisk，並在同一份 `boot.img` 的 cmdline 加入 `rdinit=/ethereal-init`。
- GKI 2.0 離線修補：只選擇一份 `init_boot.img`。補丁會將啟動檔案加入其中，把原廠 `/init` 備份為 `init.ethereal.bak`，再加入一個 `PT_LOAD`，讓 ELF 進入點先進入 Ethereal 載入器；配套 `boot.img` 及其 cmdline 保持不變。只有核心的 GKI 2.0 `boot.img` 不能作為單檔目標。Direct Install 仍會將 `init_boot` 與 `boot` 作為單一交易成對修補。

GKI 1.0 與 GKI 2.0 Direct Install 會透過 `rdinit` 先執行 `/ethereal-init`。GKI 2.0 單檔離線路徑會先進入注入原廠 `/init` 的載入器，以 `finit_module()` 載入精確相符的 KMI 模組，再跳回原 ELF 進入點。補丁不會取代原廠檔案，取消修補時會從 `init.ethereal.bak` 還原。

## 為什麼不能用同一份 KO 支援所有核心？

相同主版本的核心仍可能採用不同的 Android KMI、符號版本與 CRC。Ethereal 會為每套支援的 KMI 分別建置 KO，啟動時只載入能明確判定的版本；無法精確配對時不會猜測，系統仍會繼續啟動。

## Ethereal 與 Magisk、KernelSU 的主要差異是什麼？

Ethereal 對 GKI 1.0 與 Direct Install 使用 `rdinit` 跳板，對 GKI 2.0 單檔離線修補使用含原檔備份的 ELF 進入點 hook，並搭配每 KMI 一份的 LKM。它保留原廠 kernel Image，也不會取代原廠 `/init` 檔案，與替換 ramdisk init 或把 Root 程式碼直接編入核心原始碼的方案不同。
