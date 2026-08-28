# Pertanyaan umum

## Apa itu Ethereal?

Ethereal adalah solusi root berbasis modul kernel untuk ARM64 GKI 1.0 dan GKI 2.0. Ethereal memuat `ethereal.ko` dari ramdisk boot tanpa menulis ulang kernel Image.

## Apa yang diubah oleh patch image boot?

- GKI 1.0: `ethereal-init`, KO, dan payload boot lainnya ditambahkan ke ramdisk `boot.img`; `rdinit=/ethereal-init` ditambahkan ke cmdline `boot.img` yang sama.
- Patch offline GKI 2.0: pilih satu `init_boot.img`. Payload ditambahkan ke sana, `/init` asli disimpan sebagai `init.ethereal.bak`, lalu `PT_LOAD` tambahan mengalihkan titik masuk ELF melalui loader Ethereal. `boot.img` pasangannya beserta cmdline tidak diubah. `boot.img` GKI 2.0 yang hanya berisi kernel ditolak sebagai target tunggal. Direct Install tetap mempatch `init_boot` dan `boot` bersama-sama dalam satu transaksi.

GKI 1.0 dan Direct Install menjalankan `/ethereal-init` melalui `rdinit`. Jalur offline GKI 2.0 masuk ke loader yang disuntikkan ke `/init` asli, memuat modul KMI yang tepat dengan `finit_module()`, lalu melompat ke titik masuk ELF semula. File asli tidak diganti; unpatch memulihkannya dari `init.ethereal.bak`.

## Mengapa tidak ada satu KO universal?

Kernel dengan versi utama yang sama masih dapat memakai KMI Android, versi simbol, dan CRC yang berbeda. Ethereal membuat KO terpisah untuk setiap KMI yang didukung dan hanya memuat kecocokan yang tidak ambigu. Jika tidak ada kecocokan tepat, proses boot tetap berlanjut tanpa memuat Ethereal.
