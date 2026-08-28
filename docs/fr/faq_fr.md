# Foire aux questions

## Qu'est-ce qu'Ethereal ?

Ethereal est une solution root basée sur un module noyau pour ARM64 GKI 1.0 et GKI 2.0. Elle charge `ethereal.ko` depuis le ramdisk de démarrage sans réécrire le kernel Image.

## Que modifie le correctif de l'image de démarrage ?

- GKI 1.0 : `ethereal-init`, les KO et les autres fichiers de démarrage sont ajoutés au ramdisk de `boot.img`. `rdinit=/ethereal-init` est ajouté à la cmdline de ce même `boot.img`.
- Correctif hors ligne GKI 2.0 : un seul `init_boot.img` est sélectionné. Les fichiers y sont ajoutés, le `/init` d'origine est sauvegardé sous `init.ethereal.bak`, puis un `PT_LOAD` supplémentaire redirige son entrée ELF vers le chargeur Ethereal. Le `boot.img` correspondant et sa cmdline restent inchangés. Un `boot.img` GKI 2.0 ne contenant que le noyau est refusé comme cible autonome. Direct Install continue de corriger `init_boot` et `boot` ensemble dans une seule transaction.

GKI 1.0 et Direct Install lancent `/ethereal-init` via `rdinit`. Le chemin hors ligne GKI 2.0 entre dans le chargeur injecté dans le `/init` d'origine, charge le module KMI exact avec `finit_module()`, puis revient à l'entrée ELF d'origine. Le fichier d'origine n'est pas remplacé ; la suppression du correctif le restaure depuis `init.ethereal.bak`.

## Pourquoi n'existe-t-il pas un seul KO universel ?

Des noyaux ayant la même version principale peuvent utiliser des KMI Android, des versions de symboles et des CRC différents. Ethereal construit un KO pour chaque KMI pris en charge et ne charge qu'une correspondance non ambiguë. Sans correspondance exacte, le démarrage continue sans charger Ethereal.
