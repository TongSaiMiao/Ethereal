# Häufig gestellte Fragen

## Was ist Ethereal?

Ethereal ist eine Kernelmodul-basierte Root-Lösung für ARM64 GKI 1.0 und GKI 2.0. Es lädt `ethereal.ko` aus der Boot-Ramdisk, ohne das Kernel Image neu zu schreiben.

## Was ändert der Boot-Image-Patch?

- GKI 1.0: `ethereal-init`, die KOs und die übrige Boot-Nutzlast werden der Ramdisk von `boot.img` hinzugefügt. `rdinit=/ethereal-init` wird in die Cmdline desselben `boot.img` eingetragen.
- GKI-2.0-Offline-Patch: Es wird nur eine `init_boot.img` ausgewählt. Die Nutzlast wird dort hinzugefügt, das originale `/init` als `init.ethereal.bak` gesichert und sein ELF-Einstieg über ein zusätzliches `PT_LOAD` zum Ethereal-Loader umgeleitet. Das zugehörige `boot.img` und dessen Cmdline bleiben unverändert. Ein reines GKI-2.0-Kernel-`boot.img` wird als Einzelziel abgelehnt. Direct Install patcht `init_boot` und `boot` weiterhin gemeinsam als eine Transaktion.

GKI 1.0 und Direct Install starten `/ethereal-init` über `rdinit`. Der Offline-Pfad für GKI 2.0 betritt den in das originale `/init` injizierten Loader, lädt das exakt passende KMI-Modul mit `finit_module()` und springt anschließend zum ursprünglichen ELF-Einstieg zurück. Die Originaldatei wird nicht ersetzt; beim Entfernen des Patches wird sie aus `init.ethereal.bak` wiederhergestellt.

## Warum gibt es kein universelles KO für alle Kernel?

Kernel mit derselben Hauptversion können unterschiedliche Android-KMIs, Symbolversionen und CRCs verwenden. Ethereal baut für jedes unterstützte KMI ein eigenes KO und lädt nur eine eindeutige Übereinstimmung. Ohne exakte Zuordnung wird der Systemstart ohne Ethereal fortgesetzt.
