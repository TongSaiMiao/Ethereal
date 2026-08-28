# Pertanyaan umum

## Apa itu Ethereal?

Ethereal adalah solusi root berbasis modul kernel untuk ARM64 GKI 1.0 dan GKI 2.0. Ethereal memuat `ethereal.ko` dari ramdisk boot tanpa menulis ulang kernel Image.

## Apa yang diubah oleh patch image boot?

- GKI 1.0: `ethereal-init`, KO, dan payload boot lainnya ditambahkan ke ramdisk `boot.img`; `rdinit=/ethereal-init` ditambahkan ke cmdline `boot.img` yang sama.
- GKI 2.0: payload ditambahkan ke ramdisk `init_boot.img`, sedangkan `rdinit=/ethereal-init` ditambahkan ke cmdline `boot.img` pasangannya. Karena itu kedua image harus dipatch sebagai satu pasangan.

Kernel terlebih dahulu menjalankan `/ethereal-init`. Program ini memilih modul KMI yang tepat dari release kernel yang sedang berjalan, memuatnya dengan `finit_module()`, lalu menjalankan `/init` bawaan. Ethereal tidak mengganti `/init` dan tidak mengubah titik masuk ELF-nya.

## Mengapa tidak ada satu KO universal?

Kernel dengan versi utama yang sama masih dapat memakai KMI Android, versi simbol, dan CRC yang berbeda. Ethereal membuat KO terpisah untuk setiap KMI yang didukung dan hanya memuat kecocokan yang tidak ambigu. Jika tidak ada kecocokan tepat, proses boot tetap berlanjut tanpa memuat Ethereal.
