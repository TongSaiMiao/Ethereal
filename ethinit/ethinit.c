/* Freestanding first-stage stub.
 * finit_module the matching ethereal.<kmi>.ko, then branch to OEM e_entry.
 * No libc / no .bss: the injected PT_LOAD has p_memsz == p_filesz.
 * Keep the stack tiny — a 46KB local on 16K pages can skip the guard page
 * and SIGSEGV pid 1 (OEM splash hang) or skip the load and still jump.
 */
typedef unsigned long u64;
typedef long i64;
typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned char u8;

#include "kmi_select.h"

#define AT_FDCWD (-100)
#define O_RDONLY 0
#define O_WRONLY 1
#define S_IFCHR 0x2000
#define SYS_MKNODAT 33
#define SYS_MKDIRAT 34
#define SYS_UMOUNT2 39
#define SYS_MOUNT 40
#define SYS_OPENAT 56
#define SYS_CLOSE 57
#define SYS_LSEEK 62
#define SYS_GETDENTS64 61
#define SYS_UNAME 160
#define SYS_READ 63
#define SYS_WRITE 64
#define SYS_FINIT_MODULE 273
#define O_DIRECTORY 65536
#define DT_REG 8
#define SYS_EXECVE 221
#define SEEK_SET 0
#define MNT_DETACH 2
#define MAGIC_ORIG 0xD10E7E00E7E00001ULL
#define MAGIC_STUB 0xD10E7E00E7E00002ULL
#define MODULE_INIT_IGNORE_VERMAGIC 2

struct utsname {
	char sysname[65];
	char nodename[65];
	char release[65];
	char version[65];
	char machine[65];
	char domainname[65];
};

struct ctx {
	char release[80];
	char kmi[40];
	char mm[12];
	char path[96];
	char module_params[112];
	int kmsg;
	int loaded;
	int allow_unique_mm;
	long last_err;
};

static long sys6(long nr, long a0, long a1, long a2, long a3, long a4, long a5)
{
	register long x8 asm("x8") = nr;
	register long x0 asm("x0") = a0;
	register long x1 asm("x1") = a1;
	register long x2 asm("x2") = a2;
	register long x3 asm("x3") = a3;
	register long x4 asm("x4") = a4;
	register long x5 asm("x5") = a5;
	asm volatile("svc #0"
		     : "+r"(x0)
		     : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5)
		     : "memory", "cc");
	return x0;
}

static int is_err(long r)
{
	return r < 0 && r > -4096;
}

void *memcpy(void *d, const void *s, unsigned long n)
{
	u8 *dd = d;
	const u8 *ss = s;
	while (n--)
		*dd++ = *ss++;
	return d;
}

void *memset(void *d, int c, unsigned long n)
{
	u8 *dd = d;
	while (n--)
		*dd++ = (u8)c;
	return d;
}

void *memmove(void *d, const void *s, unsigned long n)
{
	u8 *dd = d;
	const u8 *ss = s;
	if (dd == ss || n == 0)
		return d;
	if (dd < ss) {
		while (n--)
			*dd++ = *ss++;
	} else {
		dd += n;
		ss += n;
		while (n--)
			*--dd = *--ss;
	}
	return d;
}

static unsigned slen(const char *s)
{
	unsigned n = 0;
	if (!s)
		return 0;
	while (s[n])
		n++;
	return n;
}

static void scopy(char *d, unsigned cap, const char *s)
{
	unsigned i = 0;
	if (!d || !cap)
		return;
	if (s) {
		while (s[i] && i + 1 < cap) {
			d[i] = s[i];
			i++;
		}
	}
	d[i] = 0;
}

static void scat(char *d, unsigned cap, const char *s)
{
	unsigned n = slen(d);
	unsigned i = 0;
	if (!s || n >= cap)
		return;
	while (s[i] && n + i + 1 < cap) {
		d[n + i] = s[i];
		i++;
	}
	d[n + i] = 0;
}

static int is_err_open(long fd)
{
	return is_err(fd) || fd < 0;
}

static long sys_open(const char *p, long flags)
{
	return sys6(SYS_OPENAT, AT_FDCWD, (long)p, flags, 0, 0, 0);
}

static void klog(struct ctx *c, const char *msg)
{
	char buf[192];
	unsigned n;
	if (!msg)
		return;
	scopy(buf, sizeof(buf), "ethereal-stub: ");
	scat(buf, sizeof(buf), msg);
	scat(buf, sizeof(buf), "\n");
	n = slen(buf);
	if (c->kmsg >= 0)
		sys6(SYS_WRITE, c->kmsg, (long)buf, n, 0, 0, 0);
}

static void klog2(struct ctx *c, const char *a, const char *b)
{
	char buf[192];
	scopy(buf, sizeof(buf), a);
	scat(buf, sizeof(buf), b);
	klog(c, buf);
}

static void klog_err(struct ctx *c, const char *what, long err)
{
	char buf[160];
	char num[16];
	unsigned i = 0;
	long v = err < 0 ? -err : err;
	scopy(buf, sizeof(buf), what);
	scat(buf, sizeof(buf), " err=");
	if (v == 0) {
		num[i++] = '0';
	} else {
		char tmp[16];
		unsigned t = 0;
		while (v && t < 10) {
			tmp[t++] = (char)('0' + (v % 10));
			v /= 10;
		}
		while (t)
			num[i++] = tmp[--t];
	}
	num[i] = 0;
	scat(buf, sizeof(buf), num);
	klog(c, buf);
}

static void setup_fs(struct ctx *c)
{
	/* Do not mount proc or tmpfs: FirstStageMain CHECKCALL-mounts both.
	 * A leftover mount returns EBUSY and init LOG(FATAL)s after splash.
	 */
	sys6(SYS_MKDIRAT, AT_FDCWD, (long)"/dev", 0755, 0, 0, 0);
	sys6(SYS_MKNODAT, AT_FDCWD, (long)"/dev/kmsg", S_IFCHR | 0600, 0x10b, 0,
	     0);
	c->kmsg = (int)sys_open("/dev/kmsg", O_WRONLY);
	if (is_err_open(c->kmsg))
		c->kmsg = -1;
}

static void cleanup_fs(struct ctx *c)
{
	if (c->kmsg >= 0) {
		sys6(SYS_CLOSE, c->kmsg, 0, 0, 0, 0, 0);
		c->kmsg = -1;
	}
}

/* Survives switch_root: adb shell cat this after boot.
 * 4242 = module loaded; 4100 = stub ran, no .ko; 7000+errno = finit failed.
 */
static void write_mark(unsigned v)
{
	char num[16];
	char tmp[16];
	unsigned i = 0, t = 0;
	unsigned x = v;
	long fd;

	sys6(SYS_MKDIRAT, AT_FDCWD, (long)"/proc", 0755, 0, 0, 0);
	sys6(SYS_MOUNT, (long)"proc", (long)"/proc", (long)"proc", 0, 0, 0);
	if (!x) {
		num[i++] = '0';
	} else {
		while (x && t < 10) {
			tmp[t++] = (char)('0' + (x % 10));
			x /= 10;
		}
		while (t)
			num[i++] = tmp[--t];
	}
	num[i++] = '\n';
	fd = sys_open("/proc/sys/kernel/random/write_wakeup_threshold", O_WRONLY);
	if (!is_err_open(fd)) {
		sys6(SYS_WRITE, fd, (long)num, i, 0, 0, 0);
		sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
	}
	sys6(SYS_UMOUNT2, (long)"/proc", MNT_DETACH, 0, 0, 0, 0);
}

static void parse_release(struct ctx *c)
{
	struct ethereal_kmi_selection selection;

	ethereal_parse_kmi(c->release, &selection);
	scopy(c->mm, sizeof(c->mm), selection.major_minor);
	scopy(c->kmi, sizeof(c->kmi), selection.exact);
	c->allow_unique_mm = ethereal_kmi_allows_unique_major_minor(&selection);
}

static void read_uname(struct ctx *c)
{
	struct utsname u;
	long rc;
	memset(&u, 0, sizeof(u));
	rc = sys6(SYS_UNAME, (long)&u, 0, 0, 0, 0, 0);
	if (is_err(rc))
		return;
	scopy(c->release, sizeof(c->release), u.release);
}

static int try_finit(struct ctx *c, const char *path, const char *params)
{
	long fd, rc;
	int flags;

	if (!params)
		params = "";
	fd = sys_open(path, O_RDONLY);
	if (is_err_open(fd))
		return 0;
	klog2(c, "finit ", path);
	flags = 0;
	rc = sys6(SYS_FINIT_MODULE, fd, (long)params, flags, 0, 0, 0);
	if (is_err(rc) || rc != 0) {
		klog_err(c, "finit", rc);
		sys6(SYS_LSEEK, fd, 0, SEEK_SET, 0, 0, 0);
		/* Exact/unique KMI builds carry real symbol CRCs. Ignore only the
		 * patch-level release string; never bypass MODVERSIONS CRC checks. */
		flags = MODULE_INIT_IGNORE_VERMAGIC;
		rc = sys6(SYS_FINIT_MODULE, fd, (long)params, flags, 0, 0, 0);
	}
	sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
	if (!is_err(rc) && rc == 0) {
		klog2(c, "loaded ", path);
		c->loaded = 1;
		c->last_err = 0;
		return 1;
	}
	c->last_err = rc;
	klog_err(c, "finit", rc);
	return 0;
}

static char hex_digit(u8 value)
{
	return value < 10 ? (char)('0' + value) : (char)('a' + value - 10);
}

static int read_manager_credentials(struct ctx *c)
{
	char uid[16];
	u8 token[33];
	char token_hex[65];
	long fd, n;
	unsigned i, total;

	memset(uid, 0, sizeof(uid));
	fd = sys_open("/ethereal.manager_uid", O_RDONLY);
	if (is_err_open(fd)) {
		klog(c, "missing /ethereal.manager_uid");
		return 0;
	}
	n = sys6(SYS_READ, fd, (long)uid, sizeof(uid) - 1, 0, 0, 0);
	sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
	if (is_err(n) || n <= 0)
		return 0;
	for (i = 0; i < (unsigned)n && uid[i] != '\n' && uid[i] != '\r'; i++) {
		if (uid[i] < '0' || uid[i] > '9')
			return 0;
	}
	if (i == 0)
		return 0;
	uid[i] = 0;
	if (uid[0] == '0' && uid[1] == 0)
		return 0;

	memset(token, 0, sizeof(token));
	fd = sys_open("/ethereal.manager_token", O_RDONLY);
	if (is_err_open(fd)) {
		klog(c, "missing manager token");
		return 0;
	}
	total = 0;
	while (total < sizeof(token)) {
		n = sys6(SYS_READ, fd, (long)(token + total),
			 (long)(sizeof(token) - total), 0, 0, 0);
		if (is_err(n) || n <= 0)
			break;
		total += (unsigned)n;
	}
	sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
	if (total != 32) {
		memset(token, 0, sizeof(token));
		klog(c, "manager token invalid");
		return 0;
	}
	for (i = 0; i < 32; i++) {
		token_hex[i * 2] = hex_digit((u8)(token[i] >> 4));
		token_hex[i * 2 + 1] = hex_digit((u8)(token[i] & 0x0f));
	}
	token_hex[64] = 0;
	memset(token, 0, sizeof(token));
	scopy(c->module_params, sizeof(c->module_params), "manager_uid=");
	scat(c->module_params, sizeof(c->module_params), uid);
	scat(c->module_params, sizeof(c->module_params), " manager_token=");
	scat(c->module_params, sizeof(c->module_params), token_hex);
	memset(token_hex, 0, sizeof(token_hex));
	return 1;
}

static int file_ok(const char *path)
{
	long fd = sys_open(path, O_RDONLY);
	if (is_err_open(fd))
		return 0;
	sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
	return 1;
}

struct linux_dirent64 {
	u64 d_ino;
	i64 d_off;
	u16 d_reclen;
	u8 d_type;
	char d_name[];
};

static void stage_su(struct ctx *c)
{
	/* Shared su paths belong to whoever got there first. The module has already
	 * cached the bytes by now, so all we need here is a breadcrumb in kmsg. */
	if (file_ok("/ethereal-su")) {
		klog(c, "su /ethereal-su");
		return;
	}
	if (file_ok("/eth/su")) {
		klog(c, "su /eth/su");
		return;
	}
	if (file_ok("/debug_ramdisk/su")) {
		klog(c, "su /debug_ramdisk/su");
		return;
	}
	if (file_ok("/su"))
		klog(c, "su /su");
}

static int name_is_ko(const char *n)
{
	unsigned l = slen(n);

	if (l < 4)
		return 0;
	return n[l - 3] == '.' && n[l - 2] == 'k' && n[l - 1] == 'o';
}

static void load_extra_kos(struct ctx *c)
{
	char buf[512];
	char path[96];
	long fd, n;
	unsigned off;

	fd = sys6(SYS_OPENAT, AT_FDCWD, (long)"/modules", O_RDONLY | O_DIRECTORY,
		  0, 0, 0);
	if (is_err_open(fd))
		fd = sys_open("/modules", O_RDONLY);
	if (is_err_open(fd))
		return;
	klog(c, "scan /modules");
	for (;;) {
		n = sys6(SYS_GETDENTS64, fd, (long)buf, sizeof(buf), 0, 0, 0);
		if (is_err(n) || n <= 0)
			break;
		off = 0;
		while (off + 20 <= (unsigned)n) {
			struct linux_dirent64 *de = (void *)(buf + off);

			if (de->d_reclen < 20 || off + de->d_reclen > (unsigned)n)
				break;
			if (de->d_name[0] != '.' && name_is_ko(de->d_name)) {
				scopy(path, sizeof(path), "/modules/");
				scat(path, sizeof(path), de->d_name);
				try_finit(c, path, "");
			}
			off += de->d_reclen;
		}
	}
	sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
}

static void make_ko_path(char *dst, unsigned cap, const char *kmi)
{
	scopy(dst, cap, "/ethereal.");
	scat(dst, cap, kmi);
	scat(dst, cap, ".ko");
}

static void try_exact_kmi(struct ctx *c)
{
	if (!c->kmi[0])
		return;
	make_ko_path(c->path, sizeof(c->path), c->kmi);
	try_finit(c, c->path, c->module_params);
}

static void try_unique_mm(struct ctx *c)
{
	static const char pre[][12] = {
		"android12-", "android13-", "android14-", "android15-",
		"android16-", "android11-",
	};
	char names[8][40];
	int found = 0;
	unsigned i;
	if (!c->mm[0])
		return;
	for (i = 0; i < (unsigned)(sizeof(pre) / sizeof(pre[0])); i++) {
		char cand[40];
		char path[96];
		scopy(cand, sizeof(cand), pre[i]);
		scat(cand, sizeof(cand), c->mm);
		make_ko_path(path, sizeof(path), cand);
		if (file_ok(path)) {
			scopy(names[found], sizeof(names[0]), cand);
			found++;
			if (found >= 8)
				break;
		}
	}
	if (found == 1) {
		make_ko_path(c->path, sizeof(c->path), names[0]);
		try_finit(c, c->path, c->module_params);
		return;
	}
	if (found > 1)
		klog(c, "ambiguous kmi, skip generic load");
}

/* Survives switch_root. Default page-cluster is 3.
 * 7 = .ko loaded; 6 = stub ran, finit failed. Init rarely overwrites this.
 */
static void write_page_cluster(unsigned v)
{
	char num[2];
	long fd;

	sys6(SYS_MKDIRAT, AT_FDCWD, (long)"/proc", 0755, 0, 0, 0);
	sys6(SYS_MOUNT, (long)"proc", (long)"/proc", (long)"proc", 0, 0, 0);
	num[0] = (char)('0' + (v % 10));
	num[1] = '\n';
	fd = sys_open("/proc/sys/vm/page-cluster", O_WRONLY);
	if (!is_err_open(fd)) {
		sys6(SYS_WRITE, fd, (long)num, 2, 0, 0, 0);
		sys6(SYS_CLOSE, fd, 0, 0, 0, 0, 0);
	}
	sys6(SYS_UMOUNT2, (long)"/proc", MNT_DETACH, 0, 0, 0, 0);
}

void load_ethereal(void)
{
	struct ctx c;
	memset(&c, 0, sizeof(c));
	c.kmsg = -1;
	setup_fs(&c);
	klog(&c, "hooked FirstStageMain");
	read_uname(&c);
	parse_release(&c);
	if (!read_manager_credentials(&c)) {
		klog(&c, "manager credentials invalid; skip ethereal module");
		goto out;
	}
	klog2(&c, "osrelease=", c.release);
	klog2(&c, "kmi=", c.kmi[0] ? c.kmi : "(none)");
	klog(&c, "manager credentials loaded");
	/* The patcher creates this path only for an explicit --ko selection and
	 * removes inherited generic modules otherwise. Honor that override first. */
	try_finit(&c, "/ethereal.ko", c.module_params);
	if (!c.loaded)
		try_exact_kmi(&c);
	if (!c.loaded && c.allow_unique_mm)
		try_unique_mm(&c);
	if (!c.loaded)
		klog(&c, "no matching ethereal.ko (boot continues)");
	memset(c.module_params, 0, sizeof(c.module_params));
	stage_su(&c);
	load_extra_kos(&c);
out:
	cleanup_fs(&c);
	write_page_cluster(c.loaded ? 7u : 6u);
}

/* elfpatch rewrites these in the injected PT_LOAD copy. The /ethereal-init
 * file keeps the placeholders so we know we are a standalone pid-1 trampoline.
 */
__attribute__((used, aligned(8), section(".text")))
volatile u64 ethereal_magics[2] = { MAGIC_ORIG, MAGIC_STUB };

static void exec_oem_init(unsigned long orig_sp)
{
	static const char path[] = "/init";
	static const char path2[] = "/system/bin/init";
	char *av[2];
	char *ev[1];
	long argc = 0;
	char **argv = av;
	char **envp = ev;
	long rc;

	ev[0] = 0;
	av[0] = (char *)path;
	av[1] = 0;
	if (orig_sp) {
		argc = *(long *)orig_sp;
		if (argc >= 0 && argc < 64) {
			argv = (char **)(orig_sp + 8);
			envp = argv + argc + 1;
			argv[0] = (char *)path;
		}
	}
	rc = sys6(SYS_EXECVE, (long)path, (long)argv, (long)envp, 0, 0, 0);
	(void)rc;
	av[0] = (char *)path;
	av[1] = 0;
	sys6(SYS_EXECVE, (long)path, (long)av, (long)ev, 0, 0, 0);
	av[0] = (char *)path2;
	sys6(SYS_EXECVE, (long)path2, (long)av, (long)ev, 0, 0, 0);
	sys6(SYS_EXECVE, (long)path2, (long)av, (long)envp, 0, 0, 0);
}

/* x0 = original sp at kernel entry. Returns next e_entry, or 0 if execve'd. */
unsigned long ethereal_after(unsigned long orig_sp)
{
	u64 orig = ethereal_magics[0];
	u64 stubv = ethereal_magics[1];
	unsigned long self;

	if (orig == MAGIC_ORIG) {
		exec_oem_init(orig_sp);
		return 0;
	}
	asm volatile("adr %0, _start" : "=r"(self));
	return (unsigned long)(orig + (self - stubv));
}

/* _start is in start.S so we do not depend on gcc naked. */
