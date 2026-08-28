# Sık sorulan sorular

## Ethereal nedir?

Ethereal, ARM64 GKI 1.0 ve GKI 2.0 için kernel modülü tabanlı bir root çözümüdür. Kernel Image dosyasını yeniden yazmadan `ethereal.ko` dosyasını açılış ramdiskinden yükler.

## Açılış imajı yaması neyi değiştirir?

- GKI 1.0: `ethereal-init`, KO dosyaları ve diğer açılış içeriği `boot.img` ramdiskine eklenir; aynı `boot.img` cmdline alanına `rdinit=/ethereal-init` eklenir.
- GKI 2.0 çevrimdışı yaması: yalnızca bir `init_boot.img` seçilir. İçerik buraya eklenir, stok `/init` `init.ethereal.bak` olarak saklanır ve ek bir `PT_LOAD` ELF girişini Ethereal yükleyicisine yönlendirir. Eşleşen `boot.img` ve cmdline alanı değişmez. Yalnızca kernel içeren GKI 2.0 `boot.img` tek başına hedef olarak reddedilir. Direct Install, `init_boot` ve `boot` bölümlerini tek işlem halinde birlikte yamamaya devam eder.

GKI 1.0 ve Direct Install, `/ethereal-init` programını `rdinit` üzerinden başlatır. GKI 2.0 çevrimdışı yolu stok `/init` içine eklenen yükleyiciye girer, tam eşleşen KMI modülünü `finit_module()` ile yükler ve ardından özgün ELF girişine döner. Stok dosya değiştirilmez; yama kaldırılırken `init.ethereal.bak` geri yüklenir.

## Neden tek bir evrensel KO yok?

Aynı ana sürüme sahip kerneller farklı Android KMI, sembol sürümleri ve CRC değerleri kullanabilir. Ethereal desteklenen her KMI için ayrı bir KO derler ve yalnızca kesin eşleşmeyi yükler. Tam eşleşme bulunamazsa sistem Ethereal yüklenmeden açılmaya devam eder.
