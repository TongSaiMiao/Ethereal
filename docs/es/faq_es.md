# Preguntas frecuentes

## ¿Qué es Ethereal?

Ethereal es una solución de root basada en un módulo del kernel para ARM64 GKI 1.0 y GKI 2.0. Carga `ethereal.ko` desde la ramdisk de arranque sin reescribir el kernel Image.

## ¿Qué cambia el parche de la imagen de arranque?

- GKI 1.0: `ethereal-init`, los KO y el resto de la carga se añaden a la ramdisk de `boot.img`; `rdinit=/ethereal-init` se añade a la cmdline de ese mismo `boot.img`.
- GKI 2.0: la carga se añade a la ramdisk de `init_boot.img`, mientras que `rdinit=/ethereal-init` se añade a la cmdline del `boot.img` correspondiente. Por tanto, ambas imágenes deben parchearse como un par.

El kernel inicia `/ethereal-init`. Este selecciona el módulo KMI que coincide exactamente con la versión del kernel en ejecución, lo carga mediante `finit_module()` y después ejecuta el `/init` original. Ethereal no sustituye `/init` ni cambia su punto de entrada ELF.

## ¿Por qué no existe un único KO universal?

Los kernels con la misma versión principal pueden usar KMI de Android, versiones de símbolos y CRC diferentes. Ethereal compila un KO para cada KMI compatible y solo carga una coincidencia inequívoca. Si no hay una coincidencia exacta, el arranque continúa sin cargar Ethereal.
