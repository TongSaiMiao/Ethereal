# Tez-tez verilən suallar

## Ethereal nədir?

Ethereal ARM64 GKI 1.0 və GKI 2.0 üçün nüvə modulu əsaslı root həllidir. Kernel Image dəyişdirilmədən `ethereal.ko` açılış ramdiskindən yüklənir.

## Açılış imici yaması nəyi dəyişir?

- GKI 1.0: `ethereal-init`, KO-lar və digər açılış faylları `boot.img` ramdiskinə əlavə olunur; həmin `boot.img` cmdline sətrinə `rdinit=/ethereal-init` yazılır.
- GKI 2.0: fayllar `init_boot.img` ramdiskinə, `rdinit=/ethereal-init` isə uyğun `boot.img` cmdline sətrinə əlavə olunur. Buna görə iki imic birlikdə yamanmalıdır.

Nüvə əvvəlcə `/ethereal-init` proqramını başladır. O, işləyən nüvənin release dəyərinə dəqiq uyğun KMI modulunu seçir, `finit_module()` ilə yükləyir və sonra orijinal `/init` faylını işə salır. Ethereal `/init` faylını əvəz etmir və onun ELF giriş nöqtəsini dəyişmir.

## Niyə bütün nüvələr üçün bir universal KO yoxdur?

Eyni əsas versiyalı nüvələrdə Android KMI, simvol versiyaları və CRC-lər fərqli ola bilər. Ethereal hər dəstəklənən KMI üçün ayrıca KO qurur və yalnız birmənalı uyğunluğu yükləyir. Dəqiq uyğunluq tapılmasa, sistem Ethereal yüklənmədən açılışa davam edir.
