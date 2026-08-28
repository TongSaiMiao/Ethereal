# Tez-tez verilən suallar

## Ethereal nədir?

Ethereal ARM64 GKI 1.0 və GKI 2.0 üçün nüvə modulu əsaslı root həllidir. Kernel Image dəyişdirilmədən `ethereal.ko` açılış ramdiskindən yüklənir.

## Açılış imici yaması nəyi dəyişir?

- GKI 1.0: `ethereal-init`, KO-lar və digər açılış faylları `boot.img` ramdiskinə əlavə olunur; həmin `boot.img` cmdline sətrinə `rdinit=/ethereal-init` yazılır.
- GKI 2.0 oflayn yaması: yalnız bir `init_boot.img` seçilir. Fayllar ona əlavə edilir, orijinal `/init` `init.ethereal.bak` kimi saxlanılır və əlavə `PT_LOAD` ELF girişini Ethereal yükləyicisinə yönəldir. Uyğun `boot.img` və onun cmdline sətri dəyişmir. Yalnız nüvə olan GKI 2.0 `boot.img` tək hədəf kimi rədd edilir. Direct Install yenə `init_boot` və `boot` bölmələrini bir tranzaksiya kimi birlikdə yamayır.

GKI 1.0 və Direct Install `/ethereal-init` proqramını `rdinit` vasitəsilə başladır. Oflayn GKI 2.0 yolu orijinal `/init` daxilinə əlavə edilmiş yükləyiciyə keçir, dəqiq KMI modulunu `finit_module()` ilə yükləyir və sonra ilkin ELF girişinə qayıdır. Orijinal fayl əvəz edilmir; yama ləğv ediləndə `init.ethereal.bak` bərpa olunur.

## Niyə bütün nüvələr üçün bir universal KO yoxdur?

Eyni əsas versiyalı nüvələrdə Android KMI, simvol versiyaları və CRC-lər fərqli ola bilər. Ethereal hər dəstəklənən KMI üçün ayrıca KO qurur və yalnız birmənalı uyğunluğu yükləyir. Dəqiq uyğunluq tapılmasa, sistem Ethereal yüklənmədən açılışa davam edir.
