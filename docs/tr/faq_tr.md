# Sık sorulan sorular

## Ethereal nedir?

Ethereal, ARM64 GKI 1.0 ve GKI 2.0 için kernel modülü tabanlı bir root çözümüdür. Kernel Image dosyasını yeniden yazmadan `ethereal.ko` dosyasını açılış ramdiskinden yükler.

## Açılış imajı yaması neyi değiştirir?

- GKI 1.0: `ethereal-init`, KO dosyaları ve diğer açılış içeriği `boot.img` ramdiskine eklenir; aynı `boot.img` cmdline alanına `rdinit=/ethereal-init` eklenir.
- GKI 2.0: içerik `init_boot.img` ramdiskine, `rdinit=/ethereal-init` ise eşleşen `boot.img` cmdline alanına eklenir. Bu nedenle iki imaj birlikte yamalanmalıdır.

Kernel önce `/ethereal-init` programını başlatır. Program, çalışan kernel release değerine tam uyan KMI modülünü seçer, `finit_module()` ile yükler ve ardından stok `/init` dosyasını çalıştırır. Ethereal `/init` dosyasını değiştirmez ve ELF giriş noktasına dokunmaz.

## Neden tek bir evrensel KO yok?

Aynı ana sürüme sahip kerneller farklı Android KMI, sembol sürümleri ve CRC değerleri kullanabilir. Ethereal desteklenen her KMI için ayrı bir KO derler ve yalnızca kesin eşleşmeyi yükler. Tam eşleşme bulunamazsa sistem Ethereal yüklenmeden açılmaya devam eder.
