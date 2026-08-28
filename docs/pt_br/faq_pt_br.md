# Perguntas frequentes

## O que é Ethereal?

Ethereal é uma solução root baseada em módulo de kernel para ARM64 GKI 1.0 e GKI 2.0. Ele carrega `ethereal.ko` do ramdisk de inicialização sem reescrever o kernel Image.

## O que o patch da imagem de inicialização altera?

- GKI 1.0: `ethereal-init`, os KOs e os demais arquivos de inicialização são adicionados ao ramdisk do `boot.img`; `rdinit=/ethereal-init` é adicionado à cmdline desse mesmo `boot.img`.
- GKI 2.0: os arquivos são adicionados ao ramdisk do `init_boot.img`, enquanto `rdinit=/ethereal-init` é adicionado à cmdline do `boot.img` correspondente. Portanto, as duas imagens devem ser corrigidas como um par.

O kernel inicia `/ethereal-init` primeiro. Ele seleciona o módulo KMI que corresponde exatamente à release do kernel em execução, carrega-o com `finit_module()` e depois executa o `/init` original. Ethereal não substitui `/init` nem altera seu ponto de entrada ELF.

## Por que não existe um único KO universal?

Kernels com a mesma versão principal ainda podem usar KMIs Android, versões de símbolos e CRCs diferentes. Ethereal compila um KO para cada KMI compatível e carrega apenas uma correspondência inequívoca. Se não houver correspondência exata, a inicialização continua sem carregar Ethereal.
