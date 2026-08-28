# Foire aux questions

## Qu'est-ce qu'Ethereal ?

Ethereal est une solution root basée sur un module noyau pour ARM64 GKI 1.0 et GKI 2.0. Elle charge `ethereal.ko` depuis le ramdisk de démarrage sans réécrire le kernel Image.

## Que modifie le correctif de l'image de démarrage ?

- GKI 1.0 : `ethereal-init`, les KO et les autres fichiers de démarrage sont ajoutés au ramdisk de `boot.img`. `rdinit=/ethereal-init` est ajouté à la cmdline de ce même `boot.img`.
- GKI 2.0 : les fichiers sont ajoutés au ramdisk de `init_boot.img`, tandis que `rdinit=/ethereal-init` est ajouté à la cmdline du `boot.img` correspondant. Les deux images doivent donc être corrigées ensemble.

Le noyau lance d'abord `/ethereal-init`. Celui-ci sélectionne le module KMI correspondant exactement à la version du noyau actif, le charge avec `finit_module()`, puis exécute le `/init` d'origine. Ethereal ne remplace pas `/init` et ne modifie pas son point d'entrée ELF.

## Pourquoi n'existe-t-il pas un seul KO universel ?

Des noyaux ayant la même version principale peuvent utiliser des KMI Android, des versions de symboles et des CRC différents. Ethereal construit un KO pour chaque KMI pris en charge et ne charge qu'une correspondance non ambiguë. Sans correspondance exacte, le démarrage continue sans charger Ethereal.
