# Häufig gestellte Fragen

## Was ist Ethereal?

Ethereal ist eine Kernelmodul-basierte Root-Lösung für ARM64 GKI 1.0 und GKI 2.0. Es lädt `ethereal.ko` aus der Boot-Ramdisk, ohne das Kernel Image neu zu schreiben.

## Was ändert der Boot-Image-Patch?

- GKI 1.0: `ethereal-init`, die KOs und die übrige Boot-Nutzlast werden der Ramdisk von `boot.img` hinzugefügt. `rdinit=/ethereal-init` wird in die Cmdline desselben `boot.img` eingetragen.
- GKI 2.0: Die Nutzlast wird der Ramdisk von `init_boot.img` hinzugefügt, während `rdinit=/ethereal-init` in die Cmdline des zugehörigen `boot.img` kommt. Beide Images müssen daher gemeinsam gepatcht werden.

Der Kernel startet zunächst `/ethereal-init`. Es wählt anhand des Kernel-Release das exakt passende KMI-Modul, lädt es mit `finit_module()` und startet danach das originale `/init`. Ethereal ersetzt `/init` nicht und ändert auch dessen ELF-Einstiegspunkt nicht.

## Warum gibt es kein universelles KO für alle Kernel?

Kernel mit derselben Hauptversion können unterschiedliche Android-KMIs, Symbolversionen und CRCs verwenden. Ethereal baut für jedes unterstützte KMI ein eigenes KO und lädt nur eine eindeutige Übereinstimmung. Ohne exakte Zuordnung wird der Systemstart ohne Ethereal fortgesetzt.
