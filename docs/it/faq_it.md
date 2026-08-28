# Domande frequenti

## Cos'è Ethereal?

Ethereal è una soluzione root basata su un modulo kernel per ARM64 GKI 1.0 e GKI 2.0. Carica `ethereal.ko` dal ramdisk di avvio senza riscrivere il kernel Image.

## Cosa modifica la patch dell'immagine di avvio?

- GKI 1.0: `ethereal-init`, i KO e gli altri file di avvio vengono aggiunti al ramdisk di `boot.img`; `rdinit=/ethereal-init` viene aggiunto alla cmdline dello stesso `boot.img`.
- Patch offline GKI 2.0: si seleziona un solo `init_boot.img`. I file vengono aggiunti lì, il `/init` originale viene salvato come `init.ethereal.bak` e un `PT_LOAD` aggiuntivo reindirizza il suo ingresso ELF tramite il loader Ethereal. Il relativo `boot.img` e la sua cmdline restano invariati. Un `boot.img` GKI 2.0 contenente solo il kernel viene rifiutato come destinazione singola. Direct Install continua a modificare `init_boot` e `boot` insieme in un'unica transazione.

GKI 1.0 e Direct Install avviano `/ethereal-init` tramite `rdinit`. Il percorso offline GKI 2.0 entra nel loader iniettato nel `/init` originale, carica il modulo KMI esatto con `finit_module()` e poi torna al punto di ingresso ELF originale. Il file originale non viene sostituito; la rimozione della patch lo ripristina da `init.ethereal.bak`.

## Perché non esiste un solo KO universale?

Kernel con la stessa versione principale possono usare KMI Android, versioni dei simboli e CRC diversi. Ethereal crea un KO per ogni KMI supportato e carica solo una corrispondenza univoca. Se non trova una corrispondenza esatta, l'avvio continua senza caricare Ethereal.
