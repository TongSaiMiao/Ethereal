/* SPDX-License-Identifier: GPL-2.0-or-later */
/*
 * Copyright (C) 2023 bmax121. All Rights Reserved.
 */

#ifndef _ETHEREAL_SUPERCALL_H_
#define _ETHEREAL_SUPERCALL_H_

#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "uapi/scdefs.h"

#define ETHEREAL_IOCTL 0x45544801u
#define ETHEREAL_MAGIC2 0x45544852u
#define ETHEREAL_MANAGER_TOKEN_SIZE 32
#define ETHEREAL_FD_RETRY_DELAY_NS (500LL * 1000LL * 1000LL)

struct ethereal_sc {
    uint32_t magic;
    uint32_t cmd;
    uint64_t a2;
    uint64_t a3;
    uint64_t a4;
    int64_t ret;
    uint8_t token[ETHEREAL_MANAGER_TOKEN_SIZE];
};

/* A denied reboot syscall may terminate the caller under Android seccomp.
 * Run it in a child and pass only the resulting fd back to the manager. */
static inline int ethereal_send_fd(int socket_fd, int fd)
{
    struct msghdr message;
    struct iovec io;
    struct cmsghdr *control;
    char control_buffer[CMSG_SPACE(sizeof(int))];
    char payload = 1;

    memset(&message, 0, sizeof(message));
    memset(control_buffer, 0, sizeof(control_buffer));
    io.iov_base = &payload;
    io.iov_len = sizeof(payload);
    message.msg_iov = &io;
    message.msg_iovlen = 1;
    message.msg_control = control_buffer;
    message.msg_controllen = sizeof(control_buffer);
    control = CMSG_FIRSTHDR(&message);
    if (!control)
        return -1;
    control->cmsg_level = SOL_SOCKET;
    control->cmsg_type = SCM_RIGHTS;
    control->cmsg_len = CMSG_LEN(sizeof(fd));
    memcpy(CMSG_DATA(control), &fd, sizeof(fd));
    return sendmsg(socket_fd, &message, 0) > 0 ? 0 : -1;
}

static inline int ethereal_receive_fd(int socket_fd)
{
    struct msghdr message;
    struct iovec io;
    struct cmsghdr *control;
    char control_buffer[CMSG_SPACE(sizeof(int))];
    char payload = 0;
    int fd = -1;

    memset(&message, 0, sizeof(message));
    memset(control_buffer, 0, sizeof(control_buffer));
    io.iov_base = &payload;
    io.iov_len = sizeof(payload);
    message.msg_iov = &io;
    message.msg_iovlen = 1;
    message.msg_control = control_buffer;
    message.msg_controllen = sizeof(control_buffer);
    if (recvmsg(socket_fd, &message, MSG_CMSG_CLOEXEC) <= 0)
        return -1;
    if (message.msg_flags & (MSG_CTRUNC | MSG_TRUNC))
        return -1;
    control = CMSG_FIRSTHDR(&message);
    if (!control || control->cmsg_level != SOL_SOCKET ||
        control->cmsg_type != SCM_RIGHTS ||
        control->cmsg_len < CMSG_LEN(sizeof(fd)))
        return -1;
    memcpy(&fd, CMSG_DATA(control), sizeof(fd));
    if (fd >= 0)
        (void)fcntl(fd, F_SETFD, FD_CLOEXEC);
    return fd;
}

static inline int ethereal_fd(const uint8_t *manager_token)
{
    static int cached_fd = -1;
    static int64_t retry_after_ns;
    static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
    struct timespec now;
    int64_t now_ns = 0;
    int sockets[2];
    pid_t pid;
    int status;
    int fd;

    if (!manager_token)
        return -1;

    pthread_mutex_lock(&lock);
    if (cached_fd >= 0) {
        fd = cached_fd;
        pthread_mutex_unlock(&lock);
        return fd;
    }
    if (clock_gettime(CLOCK_MONOTONIC, &now) == 0)
        now_ns = (int64_t)now.tv_sec * 1000000000LL + now.tv_nsec;
    if (now_ns > 0 && now_ns < retry_after_ns) {
        pthread_mutex_unlock(&lock);
        return -1;
    }

    if (socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, sockets) != 0) {
        retry_after_ns = now_ns > 0 ? now_ns + ETHEREAL_FD_RETRY_DELAY_NS : 0;
        pthread_mutex_unlock(&lock);
        return -1;
    }

    pid = fork();
    if (pid == 0) {
        int child_fd = -1;

        close(sockets[0]);
        syscall(__NR_reboot, SUPERCALL_HELLO_MAGIC, ETHEREAL_MAGIC2,
                (long)manager_token, (long)&child_fd);
        if (child_fd >= 0) {
            (void)ethereal_send_fd(sockets[1], child_fd);
            close(child_fd);
        }
        close(sockets[1]);
        _exit(child_fd >= 0 ? 0 : 1);
    }

    close(sockets[1]);
    if (pid < 0) {
        close(sockets[0]);
        retry_after_ns = now_ns > 0 ? now_ns + ETHEREAL_FD_RETRY_DELAY_NS : 0;
        pthread_mutex_unlock(&lock);
        return -1;
    }

    fd = ethereal_receive_fd(sockets[0]);
    close(sockets[0]);
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) { }

    if (fd >= 0) {
        cached_fd = fd;
        retry_after_ns = 0;
    } else {
        if (clock_gettime(CLOCK_MONOTONIC, &now) == 0)
            now_ns = (int64_t)now.tv_sec * 1000000000LL + now.tv_nsec;
        retry_after_ns = now_ns > 0 ? now_ns + ETHEREAL_FD_RETRY_DELAY_NS : 0;
    }

    fd = cached_fd;
    pthread_mutex_unlock(&lock);
    return fd;
}

static inline long ethereal_call(const uint8_t *manager_token, uint32_t command,
                                 long a2, long a3, long a4)
{
    struct ethereal_sc request;
    int fd;
    int rc;

    if (!manager_token)
        return -EACCES;
    fd = ethereal_fd(manager_token);
    if (fd < 0)
        return -ENOSYS;

    memset(&request, 0, sizeof(request));
    request.magic = SUPERCALL_HELLO_MAGIC;
    request.cmd = command;
    request.a2 = (uint64_t)(unsigned long)a2;
    request.a3 = (uint64_t)(unsigned long)a3;
    request.a4 = (uint64_t)(unsigned long)a4;
    memcpy(request.token, manager_token, sizeof(request.token));

    rc = ioctl(fd, ETHEREAL_IOCTL, &request);
    if (rc < 0)
        return -errno;
    return (long)request.ret;
}

static inline long sc_hello(const uint8_t *manager_token)
{
    return ethereal_call(manager_token, SUPERCALL_HELLO, 0, 0, 0);
}

static inline long sc_su(const uint8_t *manager_token, struct su_profile *profile)
{
    if (!profile || strnlen(profile->scontext, SUPERCALL_SCONTEXT_LEN) >=
                        SUPERCALL_SCONTEXT_LEN)
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_SU,
                         (long)(uintptr_t)profile, 0, 0);
}

static inline int sc_set_module_exclude(const uint8_t *manager_token, uid_t uid,
                                        int exclude)
{
    uint32_t command = exclude ? SUPERCALL_KSTORAGE_WRITE
                               : SUPERCALL_KSTORAGE_REMOVE;
    return (int)ethereal_call(manager_token, command,
                              KSTORAGE_EXCLUDE_LIST_GROUP, (long)uid, 0);
}

static inline int sc_get_module_exclude(const uint8_t *manager_token, uid_t uid)
{
    int exclude = 0;
    long rc = ethereal_call(manager_token, SUPERCALL_KSTORAGE_READ,
                            KSTORAGE_EXCLUDE_LIST_GROUP, (long)uid,
                            (long)(uintptr_t)&exclude);
    return rc < 0 ? 0 : exclude;
}

static inline long sc_su_grant_uid(const uint8_t *manager_token,
                                   struct su_profile *profile)
{
    if (!profile || strnlen(profile->scontext, SUPERCALL_SCONTEXT_LEN) >=
                        SUPERCALL_SCONTEXT_LEN)
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_SU_GRANT_UID,
                         (long)(uintptr_t)profile, 0, 0);
}

static inline long sc_su_revoke_uid(const uint8_t *manager_token, uid_t uid)
{
    return ethereal_call(manager_token, SUPERCALL_SU_REVOKE_UID,
                         (long)uid, 0, 0);
}

static inline long sc_su_uid_nums(const uint8_t *manager_token)
{
    return ethereal_call(manager_token, SUPERCALL_SU_NUMS, 0, 0, 0);
}

static inline long sc_su_allow_uids(const uint8_t *manager_token, uid_t *uids,
                                    int capacity)
{
    if (!uids || capacity <= 0)
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_SU_LIST,
                         (long)(uintptr_t)uids, capacity, 0);
}

static inline long sc_su_uid_profile(const uint8_t *manager_token, uid_t uid,
                                     struct su_profile *profile)
{
    if (!profile)
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_SU_PROFILE, (long)uid,
                         (long)(uintptr_t)profile, 0);
}

static inline long sc_su_get_path(const uint8_t *manager_token, char *path,
                                  int capacity)
{
    if (!path || capacity <= 0)
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_SU_GET_PATH,
                         (long)(uintptr_t)path, capacity, 0);
}

static inline long sc_su_reset_path(const uint8_t *manager_token,
                                    const char *path)
{
    if (!path || !path[0])
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_SU_RESET_PATH,
                         (long)(uintptr_t)path, 0, 0);
}

static inline long sc_control_feature(const uint8_t *manager_token,
                                      const char *name, int state)
{
    if (!name || !name[0])
        return -EINVAL;
    return ethereal_call(manager_token, SUPERCALL_CONTROL_FEATURE,
                         (long)(uintptr_t)name, state, 0);
}

#endif
