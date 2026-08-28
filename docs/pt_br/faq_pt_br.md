# Perguntas frequentes

## O que é Ethereal?

Ethereal é uma solução root baseada em módulo de kernel para ARM64 GKI 1.0 e GKI 2.0. Ele carrega `ethereal.ko` do ramdisk de inicialização sem reescrever o kernel Image.

## O que o patch da imagem de inicialização altera?

- GKI 1.0: `ethereal-init`, os KOs e os demais arquivos de inicialização são adicionados ao ramdisk do `boot.img`; `rdinit=/ethereal-init` é adicionado à cmdline desse mesmo `boot.img`.
- Patch offline do GKI 2.0: selecione apenas um `init_boot.img`. Os arquivos são adicionados nele, o `/init` original é salvo como `init.ethereal.bak` e um `PT_LOAD` adicional redireciona sua entrada ELF pelo carregador do Ethereal. O `boot.img` correspondente e sua cmdline permanecem inalterados. Um `boot.img` GKI 2.0 contendo apenas o kernel é rejeitado como alvo isolado. O Direct Install ainda corrige `init_boot` e `boot` juntos em uma única transação.

O GKI 1.0 e o Direct Install iniciam `/ethereal-init` por `rdinit`. O caminho offline do GKI 2.0 entra no carregador injetado no `/init` original, carrega o módulo KMI exato com `finit_module()` e então salta para a entrada ELF original. O arquivo original não é substituído; o unpatch o restaura de `init.ethereal.bak`.

## Por que não existe um único KO universal?

Kernels com a mesma versão principal ainda podem usar KMIs Android, versões de símbolos e CRCs diferentes. Ethereal compila um KO para cada KMI compatível e carrega apenas uma correspondência inequívoca. Se não houver correspondência exata, a inicialização continua sem carregar Ethereal.
