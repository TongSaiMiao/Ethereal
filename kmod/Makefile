# Out-of-tree Ethereal LKM. Built against each GKI KMI with build-gki.sh:
#   bash kmod/build-gki.sh
# Every release module is linked against CRCs extracted from the locked
# official GKI ABI artifact (or its real vmlinux.symvers), so the kernel can
# enforce symbol-version compatibility at load time.

obj-m += ethereal.o

ethereal-source-root := $(abspath $(src)/..)
ccflags-y += -Wno-error -Wno-date-time -fno-strict-aliasing \
	-fno-stack-protector -fno-asynchronous-unwind-tables \
	-mno-outline-atomics -U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0 \
	-D__NO_FORTIFY \
	-ffile-prefix-map=$(ethereal-source-root)=/workspace/Ethereal \
	-fdebug-prefix-map=$(ethereal-source-root)=/workspace/Ethereal \
	-fmacro-prefix-map=$(ethereal-source-root)=/workspace/Ethereal

KDIR ?= /lib/modules/$(shell uname -r)/build

all:
	$(MAKE) -C $(KDIR) M=$(CURDIR) ARCH=arm64 LLVM=1 LLVM_IAS=1 modules

clean:
	$(MAKE) -C $(KDIR) M=$(CURDIR) clean
