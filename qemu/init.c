/* Pid-1 for QEMU GKI SuperCall smoke test. No Android userspace. */
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
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define MAGIC1 0x11581158u
#define MAGIC2 0x45544852u
#define ETHEREAL_IOCTL 0x45544801u
#define MANAGER_TOKEN_SIZE 32
#define MANAGER_TOKEN_HEX "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
#ifndef SYS_finit_module
#define SYS_finit_module __NR_finit_module
#endif

struct ethereal_sc {
	uint32_t magic;
	uint32_t cmd;
	uint64_t a2, a3, a4;
	int64_t ret;
	uint8_t token[MANAGER_TOKEN_SIZE];
};

static const uint8_t MANAGER_TOKEN[MANAGER_TOKEN_SIZE] = {
	0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
	0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
	0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
	0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
};
static const uint8_t WRONG_MANAGER_TOKEN[MANAGER_TOKEN_SIZE] = { 0xff };
static const uint8_t ZERO_TOKEN[MANAGER_TOKEN_SIZE];

struct su_profile {
	uid_t uid;
	uid_t to_uid;
	char scontext[0x60];
};

static void write_all(int fd, const char *buf, size_t len)
{
	while (len > 0) {
		ssize_t n = write(fd, buf, len);

		if (n > 0) {
			buf += n;
			len -= (size_t)n;
			continue;
		}
		if (n < 0 && errno == EINTR)
			continue;
		break;
	}
}

static void say(const char *s)
{
	size_t n = strlen(s);
	write_all(1, s, n);
	(void)fsync(1);
}

static void sayn(const char *s, long v)
{
	char buf[160];
	int n = snprintf(buf, sizeof(buf), "%s%ld\n", s, v);
	if (n > 0)
		write_all(1, buf, (size_t)n);
	(void)fsync(1);
}

static void bind_console(void)
{
	static const char *const paths[] = { "/dev/console", "/dev/ttyAMA0", "/dev/ttyS0", NULL };
	int i, fd;

	for (i = 0; paths[i]; i++) {
		fd = open(paths[i], O_RDWR);
		if (fd < 0)
			continue;
		dup2(fd, 0);
		dup2(fd, 1);
		dup2(fd, 2);
		if (fd > 2)
			close(fd);
		return;
	}
}

static void dump_kmsg(void)
{
	char buf[512];
	int fd, n;

	fd = open("/dev/kmsg", O_RDONLY | O_NONBLOCK);
	if (fd < 0)
		return;
	say("--- kmsg ---\n");
	while ((n = read(fd, buf, sizeof(buf) - 1)) > 0) {
		buf[n] = 0;
		write_all(1, buf, (size_t)n);
	}
	close(fd);
	say("--- kmsg end ---\n");
	(void)fsync(1);
}

static int load_ko(const char *path, const char *params)
{
	int fd, rc;

	fd = open(path, O_RDONLY);
	if (fd < 0) {
		sayn("open ko errno=", errno);
		return -1;
	}
	sayn("finit nr=", SYS_finit_module);
	rc = syscall(SYS_finit_module, fd, params, 0);
	if (rc != 0)
		sayn("finit flags0 errno=", errno);
	close(fd);
	return rc;
}

static long sc_call_args_token(int fd, const uint8_t token[MANAGER_TOKEN_SIZE],
			       uint32_t cmd, uint64_t a2, uint64_t a3,
			       uint64_t a4)
{
	struct ethereal_sc sc;
	int rc;

	memset(&sc, 0, sizeof(sc));
	sc.magic = MAGIC1;
	sc.cmd = cmd;
	sc.a2 = a2;
	sc.a3 = a3;
	sc.a4 = a4;
	if (token)
		memcpy(sc.token, token, sizeof(sc.token));
	rc = ioctl(fd, ETHEREAL_IOCTL, &sc);
	if (rc < 0)
		return -errno;
	return (long)sc.ret;
}

static long sc_call_args(int fd, uint32_t cmd, uint64_t a2, uint64_t a3,
			 uint64_t a4)
{
	return sc_call_args_token(fd, ZERO_TOKEN, cmd, a2, a3, a4);
}

static long sc_call(int fd, uint32_t cmd)
{
	return sc_call_args(fd, cmd, 0, 0, 0);
}

static long sc_call_token(int fd, const uint8_t token[MANAGER_TOKEN_SIZE],
			  uint32_t cmd)
{
	return sc_call_args_token(fd, token, cmd, 0, 0, 0);
}

static int request_fd_token(const uint8_t token[MANAGER_TOKEN_SIZE])
{
	int fd = -1;

	syscall(SYS_reboot, (long)MAGIC1, (long)MAGIC2, (long)token, (long)&fd);
	return fd;
}

static int request_fd(void)
{
	return request_fd_token(ZERO_TOKEN);
}

static int wait_test(const char *name, pid_t pid)
{
	int status = 0;

	if (pid < 0 || waitpid(pid, &status, 0) != pid || !WIFEXITED(status) ||
	    WEXITSTATUS(status) != 0) {
		say("ethereal-qemu: ");
		say(name);
		say(" FAIL\n");
		return -1;
	}
	say("ethereal-qemu: ");
	say(name);
	say(" OK\n");
	return 0;
}

static int unauthorized_child(int inherited_fd)
{
	int fd;
	long r;

	if (setuid(2001) != 0)
		return 10;
	r = sc_call(inherited_fd, 0x1000);
	if (r != -EACCES)
		return 11;
	fd = request_fd();
	if (fd >= 0)
		return 12;
	return 0;
}

static int manager_child(int inherited_fd)
{
	uid_t ids[4] = { 0 };
	int value = 1;
	int out = 0;
	int fd;
	long r;

	if (setuid(2000) != 0)
		return 20;
	/* Simulate an app taking over the manager UID, including an allowlist
	 * collision and an fd inherited from the old manager. */
	if (sc_call_token(inherited_fd, WRONG_MANAGER_TOKEN, 0x1000) != -EACCES)
		return 21;
	fd = request_fd_token(WRONG_MANAGER_TOKEN);
	if (fd >= 0)
		return 22;
	fd = request_fd_token(MANAGER_TOKEN);
	if (fd < 0)
		return 23;
	if (sc_call_token(fd, WRONG_MANAGER_TOKEN, 0x1000) != -EACCES)
		return 24;
	if (sc_call_token(fd, MANAGER_TOKEN, 0x1000) != (long)MAGIC1)
		return 25;
	r = sc_call_args_token(fd, MANAGER_TOKEN, 0x1041, 1, 12345,
			       (uint64_t)(uintptr_t)&value);
	if (r != 0)
		return 26;
	r = sc_call_args_token(fd, MANAGER_TOKEN, 0x1042, 1, 12345,
			       (uint64_t)(uintptr_t)&out);
	if (r != (long)sizeof(out) || out != 1)
		return 27;
	r = sc_call_args_token(fd, MANAGER_TOKEN, 0x1043, 1,
			       (uint64_t)(uintptr_t)ids, 4);
	if (r != 1 || ids[0] != 12345)
		return 28;
	r = sc_call_args_token(fd, MANAGER_TOKEN, 0x1044, 1, 12345, 0);
	if (r != 0)
		return 29;
	out = 1;
	r = sc_call_args_token(fd, MANAGER_TOKEN, 0x1042, 1, 12345,
			       (uint64_t)(uintptr_t)&out);
	if (r != (long)sizeof(out) || out != 0)
		return 30;
	r = sc_call_token(fd, MANAGER_TOKEN, 0x1010);
	if (r != 0 || getuid() != 0)
		return 31;
	return 0;
}

static int allowed_child(void)
{
	int value = 1;
	int fd;
	long r;

	if (setuid(2002) != 0)
		return 30;
	fd = request_fd();
	if (fd < 0 || sc_call(fd, 0x1000) != (long)MAGIC1)
		return 31;
	r = sc_call_args(fd, 0x1041, 1, 9999, (uint64_t)(uintptr_t)&value);
	if (r != -EACCES)
		return 32;
	r = sc_call(fd, 0x1010);
	if (r != 0 || getuid() != 0)
		return 33;
	return 0;
}

int main(void)
{
	int fd = -1;
	long r;

	mkdir("/proc", 0755);
	mkdir("/sys", 0755);
	mkdir("/dev", 0755);
	mount("proc", "/proc", "proc", 0, 0);
	mount("sysfs", "/sys", "sysfs", 0, 0);
	mount("devtmpfs", "/dev", "devtmpfs", 0, 0);
	bind_console();

	say("ethereal-qemu: start\n");
	if (load_ko("/ethereal.ko", "manager_uid=2000 manager_token=" MANAGER_TOKEN_HEX) != 0) {
		say("ethereal-qemu: finit FAIL\n");
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=LOAD_FAIL\n");
		goto hang;
	}
	say("ethereal-qemu: module loaded\n");
	if (access("/sys/module/ethereal", F_OK) != 0) {
		sayn("ethereal-qemu: sysfs module errno=", errno);
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=MODULE_NAME_FAIL\n");
		goto hang;
	}
	if (access("/sys/module/ethereal/parameters/manager_token", F_OK) == 0) {
		say("ETHEREAL_QEMU_RESULT=TOKEN_EXPOSED\n");
		goto hang;
	}

	fd = request_fd();
	if (fd < 0) {
		sayn("ethereal-qemu: install fd=", fd);
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=FD_FAIL\n");
		goto hang;
	}
	sayn("ethereal-qemu: fd=", fd);

	r = sc_call(fd, 0x1000);
	if (r != (long)MAGIC1) {
		sayn("ethereal-qemu: hello ret=", r);
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=HELLO_FAIL\n");
		goto hang;
	}
	say("ethereal-qemu: hello OK\n");

	if (wait_test("unauthorized uid + transferred fd rejection",
		      ({ pid_t p = fork(); if (p == 0) _exit(unauthorized_child(fd)); p; })) != 0) {
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=AUTH_FAIL\n");
		goto hang;
	}
	{
		struct su_profile profile;
		memset(&profile, 0, sizeof(profile));
		profile.uid = 2000;
		profile.to_uid = 0;
		strcpy(profile.scontext, "u:r:magisk:s0");
		r = sc_call_args(fd, 0x1100, (uint64_t)(uintptr_t)&profile, 0, 0);
		if (r != 0) {
			say("ETHEREAL_QEMU_RESULT=MANAGER_ALLOWLIST_SETUP_FAIL\n");
			goto hang;
		}
	}
	if (wait_test("manager token + uid reuse rejection + kstorage + su",
		      ({ pid_t p = fork(); if (p == 0) _exit(manager_child(fd)); p; })) != 0) {
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=MANAGER_FAIL\n");
		goto hang;
	}
	{
		struct su_profile profile;
		memset(&profile, 0, sizeof(profile));
		profile.uid = 2002;
		profile.to_uid = 0;
		strcpy(profile.scontext, "u:r:magisk:s0");
		r = sc_call_args(fd, 0x1100, (uint64_t)(uintptr_t)&profile, 0, 0);
		if (r != 0) {
			sayn("ethereal-qemu: grant ret=", r);
			say("ETHEREAL_QEMU_RESULT=GRANT_FAIL\n");
			goto hang;
		}
	}
	if (wait_test("allowed uid su + admin denial",
		      ({ pid_t p = fork(); if (p == 0) _exit(allowed_child()); p; })) != 0) {
		dump_kmsg();
		say("ETHEREAL_QEMU_RESULT=ALLOW_FAIL\n");
		goto hang;
	}
	say("ETHEREAL_QEMU_RESULT=PASS\n");
	(void)fsync(1);
	/* -no-reboot: this makes qemu-system exit instead of hanging. */
	syscall(SYS_reboot, 0xfee1dead, 672274793, 0x01234567, 0);

hang:
	(void)fsync(1);
	/* Every result above is terminal. With -no-reboot this exits QEMU, so a
	 * clear failure does not consume the full harness timeout. */
	syscall(SYS_reboot, 0xfee1dead, 672274793, 0x01234567, 0);
	for (;;)
		pause();
	return 0;
}
