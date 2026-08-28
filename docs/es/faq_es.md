# Preguntas frecuentes

## ¿Qué es Ethereal?

Ethereal es una solución de root basada en un módulo del kernel para ARM64 GKI 1.0 y GKI 2.0. Carga `ethereal.ko` desde la ramdisk de arranque sin reescribir el kernel Image.

## ¿Qué cambia el parche de la imagen de arranque?

- GKI 1.0: `ethereal-init`, los KO y el resto de la carga se añaden a la ramdisk de `boot.img`; `rdinit=/ethereal-init` se añade a la cmdline de ese mismo `boot.img`.
- Parche sin conexión de GKI 2.0: se selecciona un solo `init_boot.img`. La carga se añade allí, el `/init` original se guarda como `init.ethereal.bak` y un `PT_LOAD` adicional redirige su entrada ELF mediante el cargador de Ethereal. El `boot.img` correspondiente y su cmdline no cambian. Un `boot.img` de GKI 2.0 que solo contiene el kernel se rechaza como objetivo independiente. Direct Install sigue parcheando `init_boot` y `boot` juntos en una sola transacción.

GKI 1.0 y Direct Install inician `/ethereal-init` mediante `rdinit`. La ruta sin conexión de GKI 2.0 entra en el cargador inyectado en el `/init` original, carga el módulo KMI exacto con `finit_module()` y luego salta a la entrada ELF original. El archivo original no se sustituye; al quitar el parche se restaura desde `init.ethereal.bak`.

## ¿Por qué no existe un único KO universal?

Los kernels con la misma versión principal pueden usar KMI de Android, versiones de símbolos y CRC diferentes. Ethereal compila un KO para cada KMI compatible y solo carga una coincidencia inequívoca. Si no hay una coincidencia exacta, el arranque continúa sin cargar Ethereal.
