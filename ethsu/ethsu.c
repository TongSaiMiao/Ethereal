/* SPDX-License-Identifier: GPL-3.0-or-later */
/* Ethereal SuperCall su client and early userspace setup helper. */

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

#define SUPERCALL_HELLO_MAGIC 0x11581158u
#define ETHEREAL_MAGIC2 0x45544852u
#define ETHEREAL_IOCTL 0x45544801u
#define SUPERCALL_SU 0x1010u
#define ETHEREAL_MANAGER_TOKEN_SIZE 32
#define MODULE_INIT_IGNORE_VERMAGIC 2u

#ifndef SYS_finit_module
#define SYS_finit_module 273
#endif

struct ethereal_sc {
	uint32_t magic;
	uint32_t cmd;
	uint64_t a2;
	uint64_t a3;
	uint64_t a4;
	int64_t ret;
	uint8_t token[ETHEREAL_MANAGER_TOKEN_SIZE];
};

static int ethereal_fd(void)
{
	static const uint8_t zero_token[ETHEREAL_MANAGER_TOKEN_SIZE];
	int fd = -1;

	errno = 0;
	syscall(SYS_reboot, (long)SUPERCALL_HELLO_MAGIC,
		(long)ETHEREAL_MAGIC2, (long)zero_token, (long)&fd);
	if (fd >= 0)
		(void)fcntl(fd, F_SETFD, FD_CLOEXEC);
	return fd;
}

static long ethereal_call(int fd, uint32_t cmd, uint64_t a2, uint64_t a3)
{
	struct ethereal_sc sc;
	int rc;

	memset(&sc, 0, sizeof(sc));
	sc.magic = SUPERCALL_HELLO_MAGIC;
	sc.cmd = cmd;
	sc.a2 = a2;
	sc.a3 = a3;
	rc = ioctl(fd, ETHEREAL_IOCTL, &sc);
	if (rc < 0)
		return -errno;
	return (long)sc.ret;
}

static int become_root(void)
{
	int fd;
	long rc;

	if (getuid() == 0)
		return 0;
	fd = ethereal_fd();
	if (fd < 0) {
		fprintf(stderr, "ethsu: SuperCall fd failed errno=%d\n", errno);
		return -1;
	}
	rc = ethereal_call(fd, SUPERCALL_SU, 0, 0);
	close(fd);
	if (rc != 0 || getuid() != 0) {
		fprintf(stderr, "ethsu: SuperCall SU failed ret=%ld uid=%ld\n",
			rc, (long)getuid());
		return -1;
	}
	return 0;
}

static void set_scon(const char *ctx)
{
	static const char *const attrs[] = {
		"/proc/self/attr/current",
		"/proc/thread-self/attr/current",
	};
	size_t i;

	if (!ctx || !ctx[0])
		return;
	for (i = 0; i < sizeof(attrs) / sizeof(attrs[0]); ++i) {
		int fd = open(attrs[i], O_WRONLY | O_CLOEXEC);

		if (fd < 0)
			continue;
		(void)write(fd, ctx, strlen(ctx));
		close(fd);
	}
}

static int is_opt(const char *arg, const char *short_opt, const char *long_opt)
{
	return strcmp(arg, short_opt) == 0 ||
		(long_opt && strcmp(arg, long_opt) == 0);
}

static int mkdir_if_needed(const char *path, mode_t mode)
{
	if (mkdir(path, mode) == 0 || errno == EEXIST)
		return 0;
	fprintf(stderr, "ethsu: mkdir %s failed errno=%d\n", path, errno);
	return -1;
}

static int copy_file(const char *src, const char *dst)
{
	char buf[8192];
	int in_fd;
	int out_fd;
	ssize_t n;

	in_fd = open(src, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
	if (in_fd < 0)
		return -1;
	out_fd = open(dst, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC | O_NOFOLLOW,
		0755);
	if (out_fd < 0) {
		close(in_fd);
		return -1;
	}
	while ((n = read(in_fd, buf, sizeof(buf))) > 0) {
		char *pos = buf;

		while (n > 0) {
			ssize_t written = write(out_fd, pos, (size_t)n);

			if (written <= 0) {
				close(in_fd);
				close(out_fd);
				return -1;
			}
			pos += written;
			n -= written;
		}
	}
	close(in_fd);
	if (n < 0) {
		close(out_fd);
		return -1;
	}
	(void)fchmod(out_fd, 0755);
	close(out_fd);
	return 0;
}

static int has_ko_suffix(const char *name)
{
	size_t len = strlen(name);

	return len > 3 && strcmp(name + len - 3, ".ko") == 0;
}

static int is_ethereal_module(const char *name)
{
	return strncmp(name, "ethereal", sizeof("ethereal") - 1) == 0;
}

static void finit_one(const char *path, const char *name)
{
	int fd;
	long rc;
	int saved_errno;

	if (is_ethereal_module(name))
		return;
	fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
	if (fd < 0)
		return;
	rc = syscall(SYS_finit_module, fd, "", 0);
	saved_errno = errno;
	if (rc != 0 && saved_errno != EEXIST) {
		(void)lseek(fd, 0, SEEK_SET);
		/* Preserve MODVERSIONS; only the patch-level release may differ. */
		rc = syscall(SYS_finit_module, fd, "", MODULE_INIT_IGNORE_VERMAGIC);
		saved_errno = errno;
	}
	close(fd);
	if (rc == 0)
		fprintf(stderr, "ethsu: loaded %s\n", path);
	else if (saved_errno != EEXIST)
		fprintf(stderr, "ethsu: load %s failed errno=%d\n", path,
			saved_errno);
}

static void walk_kos(const char *dir, int module_root)
{
	DIR *stream;
	struct dirent *entry;

	stream = opendir(dir);
	if (!stream)
		return;
	while ((entry = readdir(stream)) != NULL) {
		char path[512];
		struct stat st;
		int len;

		if (entry->d_name[0] == '.')
			continue;
		len = snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name);
		if (len < 0 || (size_t)len >= sizeof(path) || lstat(path, &st) != 0)
			continue;
		if (S_ISREG(st.st_mode) && has_ko_suffix(entry->d_name)) {
			finit_one(path, entry->d_name);
			continue;
		}
		if (!module_root || !S_ISDIR(st.st_mode))
			continue;
		len = snprintf(path, sizeof(path), "%s/%s/disable", dir,
			entry->d_name);
		if (len < 0 || (size_t)len >= sizeof(path) || access(path, F_OK) == 0)
			continue;
		len = snprintf(path, sizeof(path), "%s/%s/remove", dir,
			entry->d_name);
		if (len < 0 || (size_t)len >= sizeof(path) || access(path, F_OK) == 0)
			continue;
		len = snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name);
		if (len >= 0 && (size_t)len < sizeof(path))
			walk_kos(path, 0);
		len = snprintf(path, sizeof(path), "%s/%s/ethereal", dir,
			entry->d_name);
		if (len >= 0 && (size_t)len < sizeof(path))
			walk_kos(path, 0);
	}
	closedir(stream);
}

static int setup_system_su(void)
{
	static const char overlay_options[] =
		"lowerdir=/system/bin,upperdir=/dev/.ethereal/upper,"
		"workdir=/dev/.ethereal/work";
	const char *source = "/dev/.ethereal/su";

	if (mkdir_if_needed("/data/adb", 0755) != 0 ||
		mkdir_if_needed("/data/adb/eth", 0700) != 0 ||
		mkdir_if_needed("/data/adb/modules", 0755) != 0 ||
		mkdir_if_needed("/dev/.ethereal", 0700) != 0 ||
		mkdir_if_needed("/dev/.ethereal/upper", 0700) != 0 ||
		mkdir_if_needed("/dev/.ethereal/work", 0700) != 0)
		return -1;
	if (access(source, X_OK) != 0) {
		fprintf(stderr, "ethsu: missing %s\n", source);
		return -1;
	}
	if (copy_file(source, "/dev/.ethereal/upper/su") != 0) {
		fprintf(stderr, "ethsu: failed to stage overlay binaries\n");
		return -1;
	}
	if (access("/system/bin/su", X_OK) != 0 &&
		mount("ethereal-su", "/system/bin", "overlay", 0,
			overlay_options) != 0 && errno != EBUSY) {
		fprintf(stderr, "ethsu: overlay /system/bin failed errno=%d\n", errno);
		return -1;
	}
	walk_kos("/modules", 0);
	walk_kos("/data/adb/modules", 1);
	return 0;
}

int main(int argc, char **argv)
{
	const char *shell = "/system/bin/sh";
	const char *context = NULL;
	char *command = NULL;
	int i = 1;

	if (argc >= 2 && strcmp(argv[1], "--setup") == 0) {
		if (become_root() != 0)
			return 1;
		return setup_system_su() == 0 ? 0 : 1;
	}
	if (become_root() != 0)
		return 1;
	(void)chmod("/dev/.ethereal/su", 0755);
	(void)chmod("/system/bin/su", 0755);
	(void)chmod("/data/adb/eth/su", 0755);
	set_scon("u:r:magisk:s0");

	if (i < argc && argv[i][0] != '-') {
		if (strcmp(argv[i], "su") == 0 || strcmp(argv[i], "root") == 0 ||
			(argv[i][0] >= '0' && argv[i][0] <= '9'))
			i++;
		else if (strcmp(argv[i], "-c") != 0)
			i++;
	}
	while (i < argc) {
		if (is_opt(argv[i], "-c", "--command")) {
			if (++i >= argc) {
				fprintf(stderr, "ethsu: -c requires an argument\n");
				return 1;
			}
			command = argv[i++];
			continue;
		}
		if (is_opt(argv[i], "-Z", "--context")) {
			if (++i >= argc) {
				fprintf(stderr, "ethsu: -Z requires an argument\n");
				return 1;
			}
			context = argv[i++];
			continue;
		}
		if (is_opt(argv[i], "-mm", "--mount-master") ||
			is_opt(argv[i], "-M", NULL) || is_opt(argv[i], "-", NULL) ||
			is_opt(argv[i], "-l", "--login") ||
			is_opt(argv[i], "-p", "--preserve-environment")) {
			i++;
			continue;
		}
		if (is_opt(argv[i], "-s", "--shell")) {
			if (++i >= argc) {
				fprintf(stderr, "ethsu: -s requires an argument\n");
				return 1;
			}
			shell = argv[i++];
			continue;
		}
		i++;
	}
	if (context)
		set_scon(context);
	if (command) {
		char *args[] = { (char *)"sh", (char *)"-c", command, NULL };

		execv(shell, args);
	} else {
		char *args[] = { (char *)"su", NULL };

		execv(shell, args);
	}
	fprintf(stderr, "ethsu: exec %s failed errno=%d\n", shell, errno);
	return 127;
}
