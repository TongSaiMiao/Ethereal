// SPDX-License-Identifier: GPL-2.0-only
/*
 * Ethereal LKM - loaded from ramdisk by ethereal-init. The stock kernel is never rewritten.
 *
 * Per-KMI GKI SuperCall, KernelSU-style:
 *   kprobe pre on __arm64_sys_reboot (never truncate, never kretprobe, never
 *   skip-by-PC=LR). Magic reboot() queues TWA_RESUME task_work which installs
 *   an anon fd; ioctl is sleepable so commit_creds is safe.
 *
 * GKI TRIM_UNUSED_KSYMS strips unused EXPORT_SYMBOL from the Image, so
 * helpers are resolved via kallsyms_lookup_name (kprobe), not relocations.
 */
#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/kallsyms.h>
#include <linux/kprobes.h>
#include <linux/cred.h>
#include <linux/uidgid.h>
#include <linux/capability.h>
#include <linux/init.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/sched.h>
#include <linux/ptrace.h>
#include <linux/spinlock.h>
#include <linux/fs.h>
#include <linux/file.h>
#include <linux/fdtable.h>
#include <linux/anon_inodes.h>
#include <linux/task_work.h>
#include <linux/err.h>
#include <linux/uaccess.h>
#include <linux/vmalloc.h>
#include <linux/workqueue.h>
#include <linux/atomic.h>

#include "manager_cert.h"

#ifndef TWA_RESUME
#define TWA_RESUME true
#endif
#ifndef __nocfi
#define __nocfi
#endif

#define SUPERCALL_HELLO 0x1000
#define SUPERCALL_SU 0x1010
#define SUPERCALL_KSTORAGE_WRITE 0x1041
#define SUPERCALL_KSTORAGE_READ 0x1042
#define SUPERCALL_KSTORAGE_LIST_IDS 0x1043
#define SUPERCALL_KSTORAGE_REMOVE 0x1044
#define SUPERCALL_CONTROL_FEATURE 0x1046
#define SUPERCALL_SU_GRANT_UID 0x1100
#define SUPERCALL_SU_REVOKE_UID 0x1101
#define SUPERCALL_SU_NUMS 0x1102
#define SUPERCALL_SU_LIST 0x1103
#define SUPERCALL_SU_PROFILE 0x1104
#define SUPERCALL_SU_GET_PATH 0x1110
#define SUPERCALL_SU_RESET_PATH 0x1111
#define SUPERCALL_SU_GET_SAFEMODE 0x1112
#define SUPERCALL_HELLO_MAGIC 0x11581158
#define SUPERCALL_SCONTEXT_LEN 0x60
#define SU_PATH_MAX 128
#define MAX_ALLOW 128
#define MAX_EXCLUDE 128
#define KSTORAGE_EXCLUDE_LIST_GROUP 1
#define ETHEREAL_FEATURE_MARKER "ethereal-protocol-v1,uid-token-auth-v3,kstorage-v2"

#define ETHEREAL_MAGIC1 SUPERCALL_HELLO_MAGIC
#define ETHEREAL_MAGIC2 0x45544852u
#define ETHEREAL_IOCTL 0x45544801u
#define REBOOT_SYMBOL "__arm64_sys_reboot"

MODULE_LICENSE("GPL");
MODULE_AUTHOR("Ethereal");
MODULE_DESCRIPTION("Ethereal ramdisk LKM SuperCall provider");
MODULE_VERSION("1.3");
MODULE_INFO(ethereal_features, ETHEREAL_FEATURE_MARKER);


struct su_profile {
	uid_t uid;
	uid_t to_uid;
	char scontext[SUPERCALL_SCONTEXT_LEN];
};

static DEFINE_SPINLOCK(g_lock);
static struct su_profile g_allow[MAX_ALLOW];
static int g_allow_n;
static uid_t g_exclude[MAX_EXCLUDE];
static int g_exclude_n;
static unsigned int manager_uid = ~0U;
module_param_named(manager_uid, manager_uid, uint, 0400);
static char manager_token_hex[ETHEREAL_MANAGER_TOKEN_HEX_SIZE + 1];
module_param_string(manager_token, manager_token_hex,
		    sizeof(manager_token_hex), 0000);
static u8 g_manager_token[ETHEREAL_MANAGER_TOKEN_SIZE];
static bool g_manager_token_ready;
static char g_su_path[SU_PATH_MAX] = "/system/bin/su";
/* Shared with the manager and ethsu; only overwrite an iname when it fits. */
static const char g_su_real[] = "/dev/.ethereal/su";
static const char *const g_su_aliases[] = {
	"/system/bin/su", "/system/xbin/su",
	"/sbin/su", "/su/bin/su", "/debug_ramdisk/su", NULL
};
/* Appended onto atrace.rc (KernelSU vfs_read trick) so init starts ethd. */
static const char ethereal_rc[] =
	"\n"
	"on early-init\n"
	"    mkdir /dev/.ethereal 0700 root root\n"
	"on post-fs-data\n"
	"    exec u:r:init:s0 0 0 -- /system/bin/sh -c "
	"\"mkdir -p /dev/.ethereal/upper /dev/.ethereal/work;"
	"cp -f /dev/.ethereal/su /dev/.ethereal/upper/su;"
	"chmod 755 /dev/.ethereal/su /dev/.ethereal/upper/su;"
	"mount -t overlay ethereal-su -o lowerdir=/system/bin,upperdir=/dev/.ethereal/upper,workdir=/dev/.ethereal/work /system/bin\"\n"
	"    exec u:r:init:s0 0 0 -- /data/adb/ethd post-fs-data\n"
	"on nonencrypted\n"
	"    exec u:r:init:s0 0 0 -- /data/adb/ethd services\n"
	"on property:vold.decrypt=trigger_restart_framework\n"
	"    exec u:r:init:s0 0 0 -- /data/adb/ethd services\n"
	"on property:sys.boot_completed=1\n"
	"    exec u:r:init:s0 0 0 -- /data/adb/ethd boot-completed\n";

struct ethereal_sc {
	unsigned int magic;
	unsigned int cmd;
	unsigned long a2, a3, a4;
	long ret;
	u8 token[ETHEREAL_MANAGER_TOKEN_SIZE];
};

struct ethereal_fd_tw {
	struct callback_head cb;
	int __user *outp;
	const u8 __user *tokenp;
};

static struct kprobe reboot_kp;
static bool hooked_reboot;
static struct kprobe exec_kp;
static bool hooked_exec;
static int exec_filename_reg; /* 0 = x0, 1 = x1 */
static struct kretprobe vfs_read_krp;
static bool hooked_vfs;
static bool rc_injected;

typedef unsigned long (*kln_t)(const char *name);
static kln_t kln;
static int (*fn_register_kprobe)(struct kprobe *);
static void (*fn_unregister_kprobe)(struct kprobe *);
static int (*fn_register_kretprobe)(struct kretprobe *);
static void (*fn_unregister_kretprobe)(struct kretprobe *);

static noinline void *__nocfi klookup(const char *name)
{
	if (!kln || !name)
		return NULL;
	return (void *)kln(name);
}

/*
 * Pointer types must match kernel prototypes exactly: kCFI hashes the
 * function type, and a mismatched typedef panics on the indirect call.
 */
static long (*fn_strncpy_user)(char *to, const char __user *from, long n);
static typeof(get_unused_fd_flags) *fn_getfd;
static typeof(put_unused_fd) *fn_putfd;
static typeof(fd_install) *fn_install;
static typeof(anon_inode_getfile) *fn_anon;
static typeof(task_work_add) *fn_twa;
static typeof(prepare_creds) *fn_prepare_creds;
static typeof(commit_creds) *fn_commit_creds;
static struct file *(*fn_filp_open)(const char *, int, umode_t);
static int (*fn_filp_close)(struct file *, fl_owner_t);
static ssize_t (*fn_kread)(struct file *, void *, size_t, loff_t *);
static ssize_t (*fn_kwrite)(struct file *, const void *, size_t, loff_t *);
static void *(*fn_vmalloc)(unsigned long);
static void (*fn_vfree)(const void *);
static void *su_blob;
static size_t su_len;
static bool su_out;
static atomic_t su_materializing = ATOMIC_INIT(0);
static int param_cached;
static int param_placed;
static int param_exec;
static int param_vfs;
static int param_work;
static int exec_seen;
static int overlay_ok;
module_param_named(cached, param_cached, int, 0444);
module_param_named(placed, param_placed, int, 0444);
module_param_named(exec, param_exec, int, 0444);
module_param_named(vfs, param_vfs, int, 0444);
module_param_named(work, param_work, int, 0444);
module_param_named(execs, exec_seen, int, 0444);
static int param_allow;
static int param_umh;
module_param_named(allow, param_allow, int, 0444);
module_param_named(umh, param_umh, int, 0444);

static struct work_struct su_work;
static void (*fn_msleep)(unsigned int);
static int (*fn_umh)(const char *path, char **argv, char **envp, int wait);

static unsigned xstrnlen(const char *s, unsigned max)
{
	unsigned n = 0;

	if (!s)
		return 0;
	while (n < max && s[n])
		n++;
	return n;
}

static void xstrncpy(char *d, const char *s, unsigned n)
{
	unsigned i = 0;

	if (!d || !n)
		return;
	if (s) {
		while (s[i] && i + 1 < n) {
			d[i] = s[i];
			i++;
		}
	}
	while (i < n)
		d[i++] = 0;
}

static int __nocfi ethereal_register_kprobe(struct kprobe *kp)
{
	if (!fn_register_kprobe)
		return -ENOSYS;
	return fn_register_kprobe(kp);
}

static void __nocfi ethereal_unregister_kprobe(struct kprobe *kp)
{
	if (fn_unregister_kprobe)
		fn_unregister_kprobe(kp);
}

static int __nocfi ethereal_register_kretprobe(struct kretprobe *rp)
{
	if (!fn_register_kretprobe)
		return -ENOSYS;
	return fn_register_kretprobe(rp);
}

static void __nocfi ethereal_unregister_kretprobe(struct kretprobe *rp)
{
	if (fn_unregister_kretprobe)
		fn_unregister_kretprobe(rp);
}

#ifdef ETHEREAL_KLN_VIA_SPRINT_SYMBOL
#define ETHEREAL_KLN_SCAN_LIMIT (128UL * 1024UL)
#define ETHEREAL_KLN_SCAN_STEP 4UL
#define ETHEREAL_KLN_MAX_THUNK_HOPS 3U
#define ETHEREAL_AARCH64_B_MASK 0xfc000000U
#define ETHEREAL_AARCH64_B 0x14000000U
#define ETHEREAL_AARCH64_BTI_C 0xd503245fU
#define ETHEREAL_AARCH64_ADRP_X16_MASK 0x9f00001fU
#define ETHEREAL_AARCH64_ADRP_X16 0x90000010U
#define ETHEREAL_AARCH64_ADD_X16_MASK 0xffc003ffU
#define ETHEREAL_AARCH64_ADD_X16 0x91000210U
#define ETHEREAL_AARCH64_BR_X16 0xd61f0200U

static unsigned long __init ethereal_follow_branch_thunk(unsigned long address)
{
	unsigned long branch = address;
	u32 instruction = READ_ONCE(*(u32 *)address);
	s32 offset;

	if (instruction == ETHEREAL_AARCH64_BTI_C) {
		branch += sizeof(instruction);
		instruction = READ_ONCE(*(u32 *)branch);
	}
	if ((instruction & ETHEREAL_AARCH64_B_MASK) != ETHEREAL_AARCH64_B)
		return address;

	/* Sign-extend imm26 and convert the instruction offset to bytes. */
	offset = (s32)((instruction & ~ETHEREAL_AARCH64_B_MASK) << 6) >> 4;
	return (unsigned long)((long)branch + offset);
}

static unsigned long __init ethereal_follow_module_plt(unsigned long address)
{
	const u32 *entry = (const u32 *)address;
	u32 adrp = READ_ONCE(entry[0]);
	u32 add = READ_ONCE(entry[1]);
	u32 branch = READ_ONCE(entry[2]);
	u32 immediate;
	long page_delta;
	unsigned long page;

	if ((adrp & ETHEREAL_AARCH64_ADRP_X16_MASK) !=
		    ETHEREAL_AARCH64_ADRP_X16 ||
	    (add & ETHEREAL_AARCH64_ADD_X16_MASK) !=
		    ETHEREAL_AARCH64_ADD_X16 ||
	    branch != ETHEREAL_AARCH64_BR_X16)
		return address;

	immediate = ((adrp >> 5) & 0x7ffffU) << 2;
	immediate |= (adrp >> 29) & 0x3U;
	page_delta = immediate;
	if (immediate & BIT(20))
		page_delta -= BIT(21);
	page = address & ~0xfffUL;
	return (unsigned long)((long)page + page_delta * 4096L +
			       ((add >> 10) & 0xfffU));
}

static bool __init ethereal_is_kln_start(unsigned long address)
{
	static const char exact[] = "kallsyms_lookup_name+0x0/";
	char symbol[KSYM_SYMBOL_LEN];

	memset(symbol, 0, sizeof(symbol));
	sprint_symbol(symbol, address);
	return !strncmp(symbol, exact, sizeof(exact) - 1);
}

static kln_t __init resolve_kln_v54(void)
{
	unsigned long anchor = (unsigned long)sprint_symbol;
	unsigned long resolved;
	unsigned long delta;
	unsigned int hop;

	/* Unwrap only the fixed ACK CFI thunks and module PLT entries. */
	for (hop = 0; hop < ETHEREAL_KLN_MAX_THUNK_HOPS; hop++) {
		resolved = ethereal_follow_branch_thunk(anchor);
		if (resolved == anchor)
			resolved = ethereal_follow_module_plt(anchor);
		if (resolved == anchor)
			break;
		anchor = resolved;
	}

	for (delta = 0; delta <= ETHEREAL_KLN_SCAN_LIMIT;
	     delta += ETHEREAL_KLN_SCAN_STEP) {
		if (ethereal_is_kln_start(anchor + delta))
			return (kln_t)(anchor + delta);
		if (delta && ethereal_is_kln_start(anchor - delta))
			return (kln_t)(anchor - delta);
	}
	return NULL;
}
#endif

static kln_t __init resolve_kln(void)
{
#ifdef ETHEREAL_KLN_VIA_SPRINT_SYMBOL
	return resolve_kln_v54();
#else
	struct kprobe kp = { .symbol_name = "kallsyms_lookup_name" };
	kln_t fn = NULL;

	if (!register_kprobe(&kp)) {
		fn = (kln_t)kp.addr;
		unregister_kprobe(&kp);
	}
	return fn;
#endif
}

static bool __init resolve_kprobe_api(void)
{
#ifdef ETHEREAL_KLN_VIA_SPRINT_SYMBOL
	fn_register_kprobe = klookup("register_kprobe");
	fn_unregister_kprobe = klookup("unregister_kprobe");
	fn_register_kretprobe = klookup("register_kretprobe");
	fn_unregister_kretprobe = klookup("unregister_kretprobe");
#else
	fn_register_kprobe = register_kprobe;
	fn_unregister_kprobe = unregister_kprobe;
	fn_register_kretprobe = register_kretprobe;
	fn_unregister_kretprobe = unregister_kretprobe;
#endif
	return fn_register_kprobe && fn_unregister_kprobe &&
	       fn_register_kretprobe && fn_unregister_kretprobe;
}

static void resolve_helpers(void)
{
	fn_strncpy_user = klookup("strncpy_from_user");
	fn_getfd = klookup("get_unused_fd_flags");
	fn_putfd = klookup("put_unused_fd");
	fn_install = klookup("fd_install");
	fn_anon = klookup("anon_inode_getfile");
	fn_twa = klookup("task_work_add");
	fn_prepare_creds = klookup("prepare_creds");
	fn_commit_creds = klookup("commit_creds");
	fn_filp_open = klookup("filp_open");
	fn_filp_close = klookup("filp_close");
	fn_kread = klookup("kernel_read");
	fn_kwrite = klookup("kernel_write");
	fn_vmalloc = klookup("vmalloc");
	fn_vfree = klookup("vfree");
	fn_msleep = klookup("msleep");
	fn_umh = klookup("call_usermodehelper");
	param_umh = fn_umh ? 1 : 0;
}

static int __nocfi u_copy_from(void *dst, const void __user *src, unsigned long n)
{
	if (!dst || !src)
		return -EFAULT;
	return copy_from_user(dst, src, n) ? -EFAULT : 0;
}

static int __nocfi u_copy_to(void __user *dst, const void *src, unsigned long n)
{
	if (!dst || !src)
		return -EFAULT;
	return copy_to_user(dst, src, n) ? -EFAULT : 0;
}

static int u_copy_from_inatomic(void *dst, const void __user *src,
				unsigned long n)
{
	if (!dst || !src || !access_ok(src, n))
		return -EFAULT;
	return __copy_from_user_inatomic(dst, src, n) ? -EFAULT : 0;
}

static int u_copy_to_inatomic(void __user *dst, const void *src,
			      unsigned long n)
{
	if (!dst || !src || !access_ok(dst, n))
		return -EFAULT;
	return __copy_to_user_inatomic(dst, src, n) ? -EFAULT : 0;
}

static bool uid_is_manager(uid_t uid)
{
	return manager_uid != ~0U && uid == manager_uid;
}

static bool uid_in_allowlist(uid_t uid)
{
	int i;
	bool allowed = false;

	spin_lock(&g_lock);
	for (i = 0; i < g_allow_n; i++) {
		if (g_allow[i].uid == uid) {
			allowed = true;
			break;
		}
	}
	spin_unlock(&g_lock);
	return allowed;
}

static bool uid_may_request_fd(uid_t uid)
{
	return uid == 0 || uid_is_manager(uid) || uid_in_allowlist(uid);
}

static bool allowed_uid_command(unsigned int cmd)
{
	switch (cmd) {
	case SUPERCALL_HELLO:
	case SUPERCALL_SU:
		return true;
	default:
		return false;
	}
}

static long __nocfi u_strncpy_from(char *dst, const char __user *src, long n)
{
	if (!fn_strncpy_user || !dst || !src || n <= 0)
		return -EFAULT;
	return fn_strncpy_user(dst, src, n);
}

static long __nocfi handle_cmd(uid_t caller, unsigned int sc, unsigned long a2,
		       unsigned long a3, unsigned long a4);

static long __nocfi ethereal_ioctl(struct file *filp, unsigned int cmd, unsigned long arg)
{
	struct ethereal_sc sc;
	uid_t uid = current_uid().val;

	(void)filp;
	if (cmd != ETHEREAL_IOCTL)
		return -ENOTTY;
	if (u_copy_from(&sc, (void __user *)arg, sizeof(sc)))
		return -EFAULT;
	if (sc.magic != SUPERCALL_HELLO_MAGIC)
		return -EINVAL;
	if (uid != 0) {
		/* A reused manager UID never falls through to the allowlist. */
		if (uid_is_manager(uid)) {
			if (!g_manager_token_ready ||
			    !ethereal_manager_token_equal(sc.token, g_manager_token))
				return -EACCES;
		} else {
			if (!uid_in_allowlist(uid) ||
			    !allowed_uid_command(sc.cmd & 0xFFFF))
				return -EACCES;
		}
	}
	sc.ret = handle_cmd(uid, sc.cmd & 0xFFFF, sc.a2, sc.a3, sc.a4);
	if (u_copy_to((void __user *)arg, &sc, sizeof(sc)))
		return -EFAULT;
	return 0;
}

static const struct file_operations ethereal_fops = {
	.owner = THIS_MODULE,
	.unlocked_ioctl = ethereal_ioctl,
#ifdef CONFIG_COMPAT
	.compat_ioctl = ethereal_ioctl,
#endif
};

static int __nocfi ethereal_install_fd(void)
{
	struct file *filp;
	int fd;

	if (!fn_getfd || !fn_putfd || !fn_install || !fn_anon)
		return -ENOENT;
	fd = fn_getfd(O_CLOEXEC);
	if (fd < 0)
		return fd;
	filp = fn_anon("[ethereal]", &ethereal_fops, NULL, O_RDWR | O_CLOEXEC);
	if (IS_ERR(filp)) {
		fn_putfd(fd);
		return PTR_ERR(filp);
	}
	fn_install(fd, filp);
	return fd;
}

static int __nocfi authenticate_fd_request(const u8 __user *tokenp)
{
	uid_t uid = current_uid().val;
	u8 candidate[ETHEREAL_MANAGER_TOKEN_SIZE];
	bool matches;

	if (uid == 0)
		return 0;
	if (!uid_is_manager(uid))
		return uid_in_allowlist(uid) ? 0 : -EACCES;
	if (!g_manager_token_ready || !tokenp)
		return -EACCES;
	memset(candidate, 0, sizeof(candidate));
	if (u_copy_from(candidate, tokenp, sizeof(candidate))) {
		memset(candidate, 0, sizeof(candidate));
		return -EACCES;
	}
	matches = ethereal_manager_token_equal(candidate, g_manager_token);
	memset(candidate, 0, sizeof(candidate));
	return matches ? 0 : -EACCES;
}

static void __nocfi ethereal_fd_tw(struct callback_head *cb)
{
	struct ethereal_fd_tw *tw = container_of(cb, struct ethereal_fd_tw, cb);
	int fd = authenticate_fd_request(tw->tokenp);

	if (!fd)
		fd = ethereal_install_fd();

	if (tw->outp && u_copy_to(tw->outp, &fd, sizeof(fd)))
		pr_err("ethereal: install fd copy_to_user failed\n");
	kfree(tw);
	module_put(THIS_MODULE);
}

static int __nocfi reboot_pre(struct kprobe *p, struct pt_regs *regs)
{
	struct pt_regs *sr;
	struct ethereal_fd_tw *tw;
	unsigned int magic1, magic2;
	unsigned long arg4;

	(void)p;
	if (regs->regs[0] >= 0xffff000000000000UL)
		sr = (struct pt_regs *)regs->regs[0];
	else
		sr = task_pt_regs(current);
	if (!sr)
		return 0;
	magic1 = (unsigned int)sr->regs[0];
	magic2 = (unsigned int)sr->regs[1];
	if (magic1 != ETHEREAL_MAGIC1 || magic2 != ETHEREAL_MAGIC2)
		return 0;
	if (!uid_may_request_fd(current_uid().val))
		return 0;
	if (!fn_twa)
		return 0;
	arg4 = sr->regs[3];
	tw = kzalloc(sizeof(*tw), GFP_ATOMIC);
	if (!tw)
		return 0;
	tw->outp = (int __user *)arg4;
	tw->tokenp = (const u8 __user *)sr->regs[2];
	tw->cb.func = ethereal_fd_tw;
	if (!try_module_get(THIS_MODULE)) {
		kfree(tw);
		return 0;
	}
	if (fn_twa(current, &tw->cb, TWA_RESUME)) { /* enum, not int: kCFI */
		module_put(THIS_MODULE);
		kfree(tw);
		pr_warn("ethereal: task_work_add failed\n");
	}
	return 0;
}

static long __nocfi do_su(uid_t to_uid)
{
	struct cred *newc;

	if (!fn_prepare_creds || !fn_commit_creds)
		return -ENOENT;
	newc = fn_prepare_creds();
	if (!newc)
		return -ENOMEM;
	newc->uid = KUIDT_INIT(to_uid);
	newc->euid = KUIDT_INIT(to_uid);
	newc->suid = KUIDT_INIT(to_uid);
	newc->fsuid = KUIDT_INIT(to_uid);
	newc->gid = KGIDT_INIT(to_uid);
	newc->egid = KGIDT_INIT(to_uid);
	newc->sgid = KGIDT_INIT(to_uid);
	newc->fsgid = KGIDT_INIT(to_uid);
	if (to_uid == 0) {
		newc->cap_effective = CAP_FULL_SET;
		newc->cap_permitted = CAP_FULL_SET;
		newc->cap_bset = CAP_FULL_SET;
		newc->cap_inheritable = CAP_EMPTY_SET;
		newc->securebits = 0;
	}
	return fn_commit_creds(newc);
}

static void __nocfi cache_ramdisk_su(void)
{
	static const char *const srcs[] = {
		/* Shared su paths may already have an owner. Our private copy goes first. */
		"/ethereal-su", "/eth/su", "/debug_ramdisk/su", "/su", NULL
	};
	struct file *f;
	loff_t pos, size;
	int i;

	if (!fn_filp_open || !fn_filp_close || !fn_kread || !fn_vmalloc ||
	    !fn_vfree)
		return;
	for (i = 0; srcs[i]; i++) {
		pos = 0;
		f = fn_filp_open(srcs[i], O_RDONLY, 0);
		if (!f || IS_ERR(f))
			continue;
		/* Do not dereference struct file (RANDSTRUCT). kernel_read is OK. */
		su_blob = fn_vmalloc(2 * 1024 * 1024);
		if (!su_blob) {
			fn_filp_close(f, NULL);
			return;
		}
		size = 0;
		for (;;) {
			ssize_t n;

			if ((size_t)size + 65536 > 2 * 1024 * 1024)
				break;
			n = fn_kread(f, (char *)su_blob + size, 65536, &pos);
			if (n <= 0)
				break;
			size += n;
		}
		fn_filp_close(f, NULL);
		if (size <= 0) {
			fn_vfree(su_blob);
			su_blob = NULL;
			continue;
		}
		su_len = (size_t)size;
		param_cached = (int)su_len;
		pr_info("ethereal: cached su %zu from %s\n", su_len, srcs[i]);
		return;
	}
	pr_info("ethereal: no ramdisk su to cache\n");
}

static void __nocfi save_allow(void);
static void __nocfi load_allow(void);

static void __nocfi install_system_su(void)
{
	struct file *f;
	static char *envp[] = {
		"HOME=/", "PATH=/system/bin:/system/xbin", NULL
	};
	static char *av[] = { "/dev/.ethereal/su", "--setup", NULL };

	if (fn_filp_open && fn_filp_close) {
		f = fn_filp_open("/system/bin/su", O_RDONLY, 0);
		if (f && !IS_ERR(f)) {
			fn_filp_close(f, NULL);
			overlay_ok = 1;
		}
	}
	if (fn_umh && su_out)
		fn_umh(av[0], av, envp, 2);
	if (fn_filp_open && fn_filp_close) {
		f = fn_filp_open("/system/bin/su", O_RDONLY, 0);
		if (f && !IS_ERR(f)) {
			fn_filp_close(f, NULL);
			overlay_ok = 1;
		}
	}
	load_allow();
	if (g_allow_n)
		save_allow();
}

static void __nocfi chmod_dropped_su(void)
{
	install_system_su();
}

static void __nocfi materialize_su(void)
{
	struct file *f;
	loff_t pos = 0;
	bool present = false;

	if (!su_blob || !fn_filp_open || !fn_filp_close || !fn_kwrite)
		return;
	if (atomic_cmpxchg(&su_materializing, 0, 1))
		return;
	if (su_out) {
		f = fn_filp_open(g_su_real, O_RDONLY, 0);
		if (f && !IS_ERR(f)) {
			fn_filp_close(f, NULL);
			present = true;
		}
		if (present)
			goto install;
		su_out = false;
	}
	f = fn_filp_open(g_su_real, O_CREAT | O_WRONLY | O_TRUNC, 0777);
	if (!f || IS_ERR(f))
		goto out;
	if (fn_kwrite(f, su_blob, su_len, &pos) == (ssize_t)su_len) {
		su_out = true;
		param_placed = (int)su_len;
		pr_info("ethereal: wrote /dev/.ethereal/su %zu\n", su_len);
	}
	fn_filp_close(f, NULL);

install:
	if (su_out)
		chmod_dropped_su();
out:
	atomic_set(&su_materializing, 0);
}

static void __nocfi su_work_fn(struct work_struct *w)
{
	int i;

	(void)w;
	param_work++;
	/* First-stage mounts tmpfs on /dev after we load. A too-early
	 * create lands on ramdisk /dev and is hidden by that mount.
	 */
	if (fn_msleep)
		fn_msleep(4000);
	for (i = 0; i < 8; i++) {
		materialize_su();
		if (fn_msleep)
			fn_msleep(1000);
	}
}

static void __nocfi su_tw(struct callback_head *cb)
{
	param_work++;
	kfree(cb);
	materialize_su();
	module_put(THIS_MODULE);
}

static void __nocfi queue_su_drop(void)
{
	struct callback_head *cb;

	if (!fn_twa)
		return;
	cb = kzalloc(sizeof(*cb), GFP_ATOMIC);
	if (!cb)
		return;
	cb->func = su_tw;
	if (!try_module_get(THIS_MODULE)) {
		kfree(cb);
		return;
	}
	if (fn_twa(current, cb, TWA_RESUME)) {
		module_put(THIS_MODULE);
		kfree(cb);
	}
}

static int su_alias(const char *n)
{
	int i;

	if (!n || !n[0])
		return 0;
	if (!strcmp(n, g_su_real))
		return 0;
	spin_lock(&g_lock);
	if (g_su_path[0] && !strcmp(n, g_su_path) && strcmp(n, g_su_real)) {
		spin_unlock(&g_lock);
		return 1;
	}
	spin_unlock(&g_lock);
	for (i = 0; g_su_aliases[i]; i++) {
		if (!strcmp(n, g_su_aliases[i]))
			return 1;
	}
	return 0;
}

/*
 * Rewrite struct filename in do_execveat_common / do_execve so
 * execve("/system/bin/su") opens the ramdisk binary. EROFS /system has no su.
 */
static int __nocfi rewrite_su_filename(struct filename *fn)
{
	size_t old_len;
	size_t new_len = sizeof(g_su_real) - 1;

	if (!fn || IS_ERR(fn))
		return 0;
	if (!fn->name || fn->name != fn->iname || !su_alias(fn->name))
		return 0;
	old_len = xstrnlen(fn->name, SU_PATH_MAX);
	if (old_len == SU_PATH_MAX || new_len > old_len)
		return 0;
	memcpy((void *)fn->name, g_su_real, new_len + 1);
	return 0;
}

static int __nocfi exec_pre(struct kprobe *p, struct pt_regs *regs)
{
	struct filename *fn;
	unsigned long raw;

	(void)p;
	exec_seen++;
	if (exec_seen == 16 || exec_seen == 40 || exec_seen == 80 ||
	    (exec_seen > 80 && exec_seen % 40 == 0 && !overlay_ok))
		queue_su_drop();
	raw = READ_ONCE(exec_filename_reg) ? regs->regs[1] : regs->regs[0];
	fn = (struct filename *)raw;
	return rewrite_su_filename(fn);
}

struct vfs_read_ctx {
	char __user *buf;
	size_t count;
};

static bool __nocfi user_buf_has_atrace_marker(const char __user *buf,
					       size_t len)
{
	static const char marker[] = "tracing/trace_marker";
	char sample[256];
	size_t marker_len = sizeof(marker) - 1;
	size_t scan_len = len > 8192 ? 8192 : len;
	size_t off = 0;

	while (off < scan_len) {
		size_t chunk = scan_len - off;
		size_t i;

		if (chunk > sizeof(sample))
			chunk = sizeof(sample);
		if (u_copy_from_inatomic(sample, buf + off, chunk))
			return false;
		for (i = 0; i + marker_len <= chunk; i++) {
			if (!memcmp(sample + i, marker, marker_len))
				return true;
		}
		if (chunk < sizeof(sample))
			break;
		off += sizeof(sample) - marker_len + 1;
	}
	return false;
}

static int __nocfi vfs_read_entry(struct kretprobe_instance *ri,
				  struct pt_regs *regs)
{
	struct vfs_read_ctx *ctx = (struct vfs_read_ctx *)ri->data;

	ctx->buf = NULL;
	ctx->count = 0;
	if (!rc_injected && task_pid_nr(current) == 1) {
		ctx->buf = (char __user *)regs->regs[1];
		ctx->count = (size_t)regs->regs[2];
	}
	return 0;
}

static int __nocfi vfs_read_ret(struct kretprobe_instance *ri,
				struct pt_regs *regs)
{
	struct vfs_read_ctx *ctx = (struct vfs_read_ctx *)ri->data;
	ssize_t read_len = (ssize_t)regs->regs[0];
	size_t rc_len = sizeof(ethereal_rc) - 1;

	if (rc_injected || !ctx->buf || read_len <= 0)
		return 0;
	if ((size_t)read_len > ctx->count ||
	    ctx->count - (size_t)read_len < rc_len)
		return 0;
	if (!user_buf_has_atrace_marker(ctx->buf, (size_t)read_len))
		return 0;
	if (u_copy_to_inatomic(ctx->buf + read_len, ethereal_rc, rc_len))
		return 0;
	regs->regs[0] = (unsigned long)(read_len + (ssize_t)rc_len);
	rc_injected = true;
	pr_info("ethereal: injected init rc via atrace marker\n");
	return 0;
}

static int hook_exec(void)
{
	int rc;

	memset(&exec_kp, 0, sizeof(exec_kp));
	exec_kp.symbol_name = "do_execveat_common";
	exec_kp.pre_handler = exec_pre;
	WRITE_ONCE(exec_filename_reg, 1);
	rc = ethereal_register_kprobe(&exec_kp);
	if (!rc) {
		hooked_exec = true;
		param_exec = 1;
		pr_info("ethereal: sucompat do_execveat_common\n");
		return 0;
	}

	memset(&exec_kp, 0, sizeof(exec_kp));
	exec_kp.symbol_name = "do_execve";
	exec_kp.pre_handler = exec_pre;
	WRITE_ONCE(exec_filename_reg, 0);
	rc = ethereal_register_kprobe(&exec_kp);
	if (!rc) {
		hooked_exec = true;
		param_exec = 1;
		pr_info("ethereal: sucompat do_execve\n");
		return 0;
	}
	return -ENOENT;
}

static int hook_vfs_read(void)
{
	int rc;

	memset(&vfs_read_krp, 0, sizeof(vfs_read_krp));
	vfs_read_krp.kp.symbol_name = "vfs_read";
	vfs_read_krp.entry_handler = vfs_read_entry;
	vfs_read_krp.handler = vfs_read_ret;
	vfs_read_krp.data_size = sizeof(struct vfs_read_ctx);
	vfs_read_krp.maxactive = 16;
	rc = ethereal_register_kretprobe(&vfs_read_krp);
	if (rc) {
		pr_info("ethereal: vfs_read kretprobe failed %d\n", rc);
		return rc;
	}
	hooked_vfs = true;
	param_vfs = 1;
	return 0;
}

static void put_uint(char *b, unsigned *n, unsigned cap, unsigned v)
{
	char t[12];
	int i = 0;

	if (v == 0)
		t[i++] = '0';
	while (v && i < 10) {
		t[i++] = (char)('0' + (v % 10));
		v /= 10;
	}
	while (i && *n + 1 < cap)
		b[(*n)++] = t[--i];
}

static unsigned parse_uint(const char **pp)
{
	unsigned v = 0;
	const char *p = *pp;

	while (*p == ' ' || *p == '\t')
		p++;
	while (*p >= '0' && *p <= '9') {
		v = v * 10 + (unsigned)(*p - '0');
		p++;
	}
	*pp = p;
	return v;
}

static int allow_add_locked(struct su_profile *p)
{
	int i;

	for (i = 0; i < g_allow_n; i++) {
		if (g_allow[i].uid == p->uid) {
			g_allow[i] = *p;
			return 0;
		}
	}
	if (g_allow_n >= MAX_ALLOW)
		return -ENOSPC;
	g_allow[g_allow_n++] = *p;
	return 0;
}

static void __nocfi save_allow(void)
{
	struct su_profile *tmp;
	int cnt, i;
	char *b;
	unsigned n = 0;
	struct file *f;
	loff_t pos = 0;

	if (!fn_filp_open || !fn_filp_close || !fn_kwrite || !fn_vmalloc ||
	    !fn_vfree)
		return;
	tmp = fn_vmalloc(sizeof(*tmp) * MAX_ALLOW);
	if (!tmp)
		return;
	spin_lock(&g_lock);
	cnt = g_allow_n;
	memcpy(tmp, g_allow, (size_t)cnt * sizeof(tmp[0]));
	spin_unlock(&g_lock);
	b = fn_vmalloc(8192);
	if (!b) {
		fn_vfree(tmp);
		return;
	}
	memset(b, 0, 8192);
	for (i = 0; i < cnt; i++) {
		unsigned sl;

		put_uint(b, &n, 8000, tmp[i].uid);
		if (n + 1 < 8000)
			b[n++] = ' ';
		put_uint(b, &n, 8000, tmp[i].to_uid);
		if (n + 1 < 8000)
			b[n++] = ' ';
		sl = xstrnlen(tmp[i].scontext, SUPERCALL_SCONTEXT_LEN);
		if (n + sl + 2 < 8000) {
			memcpy(b + n, tmp[i].scontext, sl);
			n += sl;
		}
		if (n + 1 < 8000)
			b[n++] = '\n';
	}
	f = fn_filp_open("/data/adb/eth/allow_uids", O_CREAT | O_WRONLY | O_TRUNC,
			 0644);
	if (f && !IS_ERR(f)) {
		fn_kwrite(f, b, n, &pos);
		fn_filp_close(f, NULL);
	}
	fn_vfree(b);
	fn_vfree(tmp);
	param_allow = cnt;
}

static void __nocfi load_allow_buf(char *b, size_t len)
{
	char *p = b, *end = b + len;

	b[len] = 0;
	while (p < end) {
		struct su_profile pr;
		const char *s;
		char *nl;
		unsigned i;

		nl = strchr(p, '\n');
		if (!nl)
			nl = end;
		*nl = 0;
		while (*p == ' ' || *p == '\t')
			p++;
		if (*p && *p != '#') {
			memset(&pr, 0, sizeof(pr));
			s = p;
			pr.uid = parse_uint(&s);
			pr.to_uid = parse_uint(&s);
			while (*s == ' ' || *s == '\t')
				s++;
			for (i = 0; i + 1 < SUPERCALL_SCONTEXT_LEN && s[i] &&
			     s[i] != ' ' && s[i] != '\r'; i++)
				pr.scontext[i] = s[i];
			if (!pr.scontext[0])
				xstrncpy(pr.scontext, "u:r:magisk:s0",
					 SUPERCALL_SCONTEXT_LEN);
			if (pr.uid) {
				spin_lock(&g_lock);
				allow_add_locked(&pr);
				param_allow = g_allow_n;
				spin_unlock(&g_lock);
			}
		}
		p = (nl < end) ? nl + 1 : end;
	}
}

static void __nocfi load_allow(void)
{
	struct file *f;
	char *b;
	loff_t pos, size;

	if (!fn_filp_open || !fn_filp_close || !fn_kread || !fn_vmalloc ||
	    !fn_vfree)
		return;
	b = fn_vmalloc(8192);
	if (!b)
		return;
	pos = 0;
	size = 0;
	memset(b, 0, 8192);
	f = fn_filp_open("/data/adb/eth/allow_uids", O_RDONLY, 0);
	if (f && !IS_ERR(f)) {
		for (;;) {
			ssize_t n;

			if ((size_t)size + 512 > 8000)
				break;
			n = fn_kread(f, b + size, 512, &pos);
			if (n <= 0)
				break;
			size += n;
		}
		fn_filp_close(f, NULL);
		if (size > 0)
			load_allow_buf(b, (size_t)size);
	}
	fn_vfree(b);
}

static bool approved_target_uid(uid_t caller, uid_t *to_uid)
{
	int i;
	bool found = false;

	spin_lock(&g_lock);
	for (i = 0; i < g_allow_n; i++) {
		if (g_allow[i].uid == caller) {
			*to_uid = g_allow[i].to_uid;
			found = true;
			break;
		}
	}
	spin_unlock(&g_lock);
	return found;
}

static long __nocfi handle_cmd(uid_t caller, unsigned int sc, unsigned long a2,
		       unsigned long a3, unsigned long a4)
{
	(void)a4;
	switch (sc) {
	case SUPERCALL_HELLO:
		return SUPERCALL_HELLO_MAGIC;
	case SUPERCALL_SU: {
		uid_t to = 0;

		if (caller != 0 && !uid_is_manager(caller)) {
			if (!approved_target_uid(caller, &to))
				return -EACCES;
		} else if (a2) {
			struct su_profile p;

			memset(&p, 0, sizeof(p));
			if (u_copy_from(&p, (void __user *)a2, sizeof(p)))
				return -EFAULT;
			to = p.to_uid;
		}
		return do_su(to);
	}
	case SUPERCALL_SU_GRANT_UID: {
		struct su_profile p;
		int rc;

		if (u_copy_from(&p, (void __user *)a2, sizeof(p)))
			return -EFAULT;
		spin_lock(&g_lock);
		rc = allow_add_locked(&p);
		param_allow = g_allow_n;
		spin_unlock(&g_lock);
		if (rc)
			return rc;
		save_allow();
		return 0;
	}
	case SUPERCALL_SU_REVOKE_UID: {
		uid_t uid = (uid_t)a2;
		int i, j;

		spin_lock(&g_lock);
		for (i = 0; i < g_allow_n; i++) {
			if (g_allow[i].uid == uid) {
				for (j = i; j < g_allow_n - 1; j++)
					g_allow[j] = g_allow[j + 1];
				g_allow_n--;
				break;
			}
		}
		param_allow = g_allow_n;
		spin_unlock(&g_lock);
		save_allow();
		return 0;
	}
	case SUPERCALL_SU_NUMS:
		return g_allow_n;
	case SUPERCALL_SU_LIST: {
		uid_t tmp[MAX_ALLOW];
		int n, i, cap = (int)a3;

		if (cap < 0 || cap > MAX_ALLOW)
			return -EINVAL;

		spin_lock(&g_lock);
		n = g_allow_n;
		for (i = 0; i < n; i++)
			tmp[i] = g_allow[i].uid;
		spin_unlock(&g_lock);
		if (cap < n)
			return -ENOSPC;
		if (n > 0 && u_copy_to((void __user *)a2, tmp, n * sizeof(uid_t)))
			return -EFAULT;
		return n;
	}
	case SUPERCALL_SU_PROFILE: {
		uid_t uid = (uid_t)a2;
		struct su_profile p;
		int i, found = 0;

		memset(&p, 0, sizeof(p));
		spin_lock(&g_lock);
		for (i = 0; i < g_allow_n; i++) {
			if (g_allow[i].uid == uid) {
				p = g_allow[i];
				found = 1;
				break;
			}
		}
		spin_unlock(&g_lock);
		if (!found) {
			p.uid = uid;
			p.to_uid = 0;
		}
		if (u_copy_to((void __user *)a3, &p, sizeof(p)))
			return -EFAULT;
		return 0;
	}
	case SUPERCALL_SU_GET_PATH: {
		char buf[SU_PATH_MAX];
		unsigned long cap = a3;
		size_t len;

		if (!cap)
			return -EINVAL;

		spin_lock(&g_lock);
		xstrncpy(buf, g_su_path, SU_PATH_MAX);
		spin_unlock(&g_lock);
		len = xstrnlen(buf, SU_PATH_MAX - 1) + 1;
		if (cap < len)
			return -ENOSPC;
		if (u_copy_to((void __user *)a2, buf, len))
			return -EFAULT;
		return len - 1;
	}
	case SUPERCALL_SU_RESET_PATH: {
		char buf[SU_PATH_MAX];
		long n;

		n = u_strncpy_from(buf, (const char __user *)a2, SU_PATH_MAX);
		if (n < 0)
			return -EFAULT;
		buf[SU_PATH_MAX - 1] = 0;
		spin_lock(&g_lock);
		xstrncpy(g_su_path, buf, SU_PATH_MAX);
		spin_unlock(&g_lock);
		return 0;
	}
	case SUPERCALL_SU_GET_SAFEMODE:
		return 0;
	case SUPERCALL_KSTORAGE_WRITE: {
		int gid = (int)a2;
		uid_t uid = (uid_t)a3;
		int i;

		if (gid != KSTORAGE_EXCLUDE_LIST_GROUP)
			return -EOPNOTSUPP;
		spin_lock(&g_lock);
		for (i = 0; i < g_exclude_n; i++) {
			if (g_exclude[i] == uid) {
				spin_unlock(&g_lock);
				return 0;
			}
		}
		if (g_exclude_n >= MAX_EXCLUDE) {
			spin_unlock(&g_lock);
			return -ENOSPC;
		}
		g_exclude[g_exclude_n++] = uid;
		spin_unlock(&g_lock);
		return 0;
	}
	case SUPERCALL_KSTORAGE_READ: {
		int gid = (int)a2;
		uid_t uid = (uid_t)a3;
		int value = 0;
		int i;

		if (gid != KSTORAGE_EXCLUDE_LIST_GROUP)
			return -EOPNOTSUPP;
		if (!a4)
			return -EINVAL;
		spin_lock(&g_lock);
		for (i = 0; i < g_exclude_n; i++) {
			if (g_exclude[i] == uid) {
				value = 1;
				break;
			}
		}
		spin_unlock(&g_lock);
		if (u_copy_to((void __user *)a4, &value, sizeof(value)))
			return -EFAULT;
		return sizeof(value);
	}
	case SUPERCALL_KSTORAGE_REMOVE: {
		int gid = (int)a2;
		uid_t uid = (uid_t)a3;
		int i, j;

		if (gid != KSTORAGE_EXCLUDE_LIST_GROUP)
			return -EOPNOTSUPP;
		spin_lock(&g_lock);
		for (i = 0; i < g_exclude_n; i++) {
			if (g_exclude[i] == uid) {
				for (j = i; j < g_exclude_n - 1; j++)
					g_exclude[j] = g_exclude[j + 1];
				g_exclude_n--;
				break;
			}
		}
		spin_unlock(&g_lock);
		return 0;
	}
	case SUPERCALL_KSTORAGE_LIST_IDS: {
		uid_t tmp[MAX_EXCLUDE];
		int n, cap = (int)a4;

		if (cap < 0 || cap > MAX_EXCLUDE)
			return -EINVAL;

		spin_lock(&g_lock);
		n = g_exclude_n;
		memcpy(tmp, g_exclude, n * sizeof(uid_t));
		spin_unlock(&g_lock);
		if (cap < n)
			return -ENOSPC;
		if (n > 0 && u_copy_to((void __user *)a3, tmp, n * sizeof(uid_t)))
			return -EFAULT;
		return n;
	}
	case SUPERCALL_CONTROL_FEATURE:
		return 0;
	default:
		return -ENOSYS;
	}
}

static int __init __nocfi ethereal_init(void)
{
	int rc = -EINVAL;

	if (manager_uid == ~0U || manager_uid == 0) {
		pr_err("ethereal: manager_uid module parameter is required\n");
		goto fail_token;
	}
	if (!ethereal_manager_token_decode(manager_token_hex, g_manager_token)) {
		pr_err("ethereal: valid manager_token module parameter is required\n");
		goto fail_token;
	}
	memset(manager_token_hex, 0, sizeof(manager_token_hex));
	g_manager_token_ready = true;
	kln = resolve_kln();
	if (!kln) {
		pr_err("ethereal: kallsyms_lookup_name missing\n");
		rc = -ENOENT;
		goto fail_token;
	}
	if (!resolve_kprobe_api()) {
		pr_err("ethereal: kprobe API missing\n");
		rc = -ENOENT;
		goto fail_token;
	}
	resolve_helpers();
	pr_info("ethereal: helpers kln=%p twa=%p anon=%p cred=%p open=%p\n",
		kln, fn_twa, fn_anon, fn_prepare_creds, fn_filp_open);

	memset(&reboot_kp, 0, sizeof(reboot_kp));
	reboot_kp.symbol_name = REBOOT_SYMBOL;
	reboot_kp.pre_handler = reboot_pre;
	rc = ethereal_register_kprobe(&reboot_kp);
	if (rc) {
		pr_err("ethereal: reboot kprobe failed %d\n", rc);
		goto fail_token;
	}
	hooked_reboot = true;
	cache_ramdisk_su();
	INIT_WORK(&su_work, su_work_fn);
	(void)hook_exec();
	(void)hook_vfs_read();
	schedule_work(&su_work);
	pr_info("ethereal: ready kprobe=1 sucompat=%d rc=%d\n",
		hooked_exec, hooked_vfs);
	return 0;

fail_token:
	memset(manager_token_hex, 0, sizeof(manager_token_hex));
	memset(g_manager_token, 0, sizeof(g_manager_token));
	g_manager_token_ready = false;
	return rc;
}

static void __exit ethereal_exit(void)
{
	if (hooked_vfs)
		ethereal_unregister_kretprobe(&vfs_read_krp);
	if (hooked_exec)
		ethereal_unregister_kprobe(&exec_kp);
	if (hooked_reboot)
		ethereal_unregister_kprobe(&reboot_kp);
	cancel_work_sync(&su_work);
	if (su_blob && fn_vfree) {
		fn_vfree(su_blob);
		su_blob = NULL;
	}
	memset(manager_token_hex, 0, sizeof(manager_token_hex));
	memset(g_manager_token, 0, sizeof(g_manager_token));
	g_manager_token_ready = false;
}

module_init(ethereal_init);
module_exit(ethereal_exit);
