# Domande frequenti

## Cos'è Ethereal?

Ethereal è una soluzione root basata su un modulo kernel per ARM64 GKI 1.0 e GKI 2.0. Carica `ethereal.ko` dal ramdisk di avvio senza riscrivere il kernel Image.

## Cosa modifica la patch dell'immagine di avvio?

- GKI 1.0: `ethereal-init`, i KO e gli altri file di avvio vengono aggiunti al ramdisk di `boot.img`; `rdinit=/ethereal-init` viene aggiunto alla cmdline dello stesso `boot.img`.
- GKI 2.0: i file vengono aggiunti al ramdisk di `init_boot.img`, mentre `rdinit=/ethereal-init` viene aggiunto alla cmdline del relativo `boot.img`. Le due immagini devono quindi essere modificate insieme.

Il kernel avvia prima `/ethereal-init`. Questo seleziona il modulo KMI che corrisponde esattamente alla release del kernel in esecuzione, lo carica con `finit_module()` e poi esegue il `/init` originale. Ethereal non sostituisce `/init` e non ne modifica il punto di ingresso ELF.

## Perché non esiste un solo KO universale?

Kernel con la stessa versione principale possono usare KMI Android, versioni dei simboli e CRC diversi. Ethereal crea un KO per ogni KMI supportato e carica solo una corrispondenza univoca. Se non trova una corrispondenza esatta, l'avvio continua senza caricare Ethereal.
