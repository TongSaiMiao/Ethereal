/* OEM /init fixture for the GKI 1.0 offline boot-patch QEMU test.
 * /ethereal-init must select the KMI-qualified module before handing pid 1
 * back to this unchanged executable.
 */
#ifndef ETHEREAL_EXPECTED_KMI
#error "ETHEREAL_EXPECTED_KMI must be a string literal"
#endif

#define main ethereal_direct_module_test_main
#include "../qemu/init.c"
#undef main

#define EXPECTED_KO_PATH "/ethereal." ETHEREAL_EXPECTED_KMI ".ko"

static int check_manager_credential_files(void)
{
	char value[16] = { 0 };
	uint8_t token[MANAGER_TOKEN_SIZE] = { 0 };
	struct stat st;
	int fd;
	ssize_t n;

	fd = open("/ethereal.manager_uid", O_RDONLY);
	if (fd < 0)
		return 40;
	if (fstat(fd, &st) != 0 || (st.st_mode & 0777) != 0400) {
		close(fd);
		return 41;
	}
	n = read(fd, value, sizeof(value) - 1);
	close(fd);
	if (n != 5 || memcmp(value, "2000\n", 5) != 0)
		return 42;

	fd = open("/ethereal.manager_token", O_RDONLY);
	if (fd < 0)
		return 43;
	if (fstat(fd, &st) != 0 || (st.st_mode & 0777) != 0400) {
		close(fd);
		return 44;
	}
	n = read(fd, token, sizeof(token));
	close(fd);
	if (n != MANAGER_TOKEN_SIZE || memcmp(token, MANAGER_TOKEN, sizeof(token)) != 0)
		return 45;
	return 0;
}

int main(void)
{
	int fd;

	mkdir("/proc", 0755);
	mkdir("/sys", 0755);
	mkdir("/dev", 0755);
	mount("proc", "/proc", "proc", 0, 0);
	mount("sysfs", "/sys", "sysfs", 0, 0);
	mount("devtmpfs", "/dev", "devtmpfs", 0, 0);
	bind_console();

	say("ethereal-gki1-e2e: OEM init handoff OK\n");
	if (access("/ethereal-init", X_OK) != 0 ||
	    access(EXPECTED_KO_PATH, R_OK) != 0 ||
	    access("/ethereal.ko", F_OK) == 0 ||
	    access("/sys/module/ethereal", F_OK) != 0 ||
	    check_manager_credential_files() != 0) {
		dump_kmsg();
		say("ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=LAYOUT_FAIL\n");
		goto hang;
	}
	say("ethereal-gki1-e2e: exact KMI " ETHEREAL_EXPECTED_KMI " KO load OK\n");

	fd = request_fd();
	if (fd < 0 || sc_call(fd, 0x1000) != (long)MAGIC1) {
		dump_kmsg();
		say("ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=ROOT_FD_FAIL\n");
		goto hang;
	}
	if (wait_test("GKI 1.0 patched ramdisk rejects unauthorized uid",
		      ({ pid_t p = fork(); if (p == 0) _exit(unauthorized_child(fd)); p; })) != 0) {
		dump_kmsg();
		say("ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=AUTH_FAIL\n");
		goto hang;
	}
	if (wait_test("GKI 1.0 manager token + kstorage + su",
		      ({ pid_t p = fork(); if (p == 0) _exit(manager_child(fd)); p; })) != 0) {
		dump_kmsg();
		say("ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=MANAGER_FAIL\n");
		goto hang;
	}

	say("ethereal-gki1-e2e: protocol and authorization OK\n");
	say("ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=PASS\n");
	(void)fsync(1);
	syscall(SYS_reboot, 0xfee1dead, 672274793, 0x01234567, 0);

hang:
	(void)fsync(1);
	syscall(SYS_reboot, 0xfee1dead, 672274793, 0x01234567, 0);
	for (;;)
		pause();
	return 0;
}
