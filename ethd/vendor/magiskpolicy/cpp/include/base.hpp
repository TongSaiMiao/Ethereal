#pragma once

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <fstream>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <sys/stat.h>
#include <unistd.h>
#include <utility>
#include <vector>

#include <rust/cxx.h>

#define LOGD(...) ((void) 0)
#define LOGI(...) std::fprintf(stderr, __VA_ARGS__)
#define LOGW(...) std::fprintf(stderr, __VA_ARGS__)
#define LOGE(...) std::fprintf(stderr, __VA_ARGS__)

inline int xopen(const char *path, int flags, mode_t mode = 0) {
    return ::open(path, flags, mode);
}

inline ssize_t xread(int fd, void *buf, size_t count) {
    size_t done = 0;
    while (done < count) {
        const ssize_t n = ::read(fd, static_cast<char *>(buf) + done, count - done);
        if (n == 0) break;
        if (n < 0) {
            if (errno == EINTR) continue;
            return n;
        }
        done += static_cast<size_t>(n);
    }
    return static_cast<ssize_t>(done);
}

inline ssize_t xwrite(int fd, const void *buf, size_t count) {
    size_t done = 0;
    while (done < count) {
        const ssize_t n = ::write(fd, static_cast<const char *>(buf) + done, count - done);
        if (n < 0) {
            if (errno == EINTR) continue;
            return n;
        }
        done += static_cast<size_t>(n);
    }
    return static_cast<ssize_t>(done);
}

inline FILE *xfopen(const char *path, const char *mode) { return std::fopen(path, mode); }
inline int xfstat(int fd, struct stat *st) { return ::fstat(fd, st); }

using sFILE = std::unique_ptr<FILE, decltype(&std::fclose)>;
inline sFILE xopen_file(const char *path, const char *mode) {
    return sFILE(std::fopen(path, mode), &std::fclose);
}

template <class Func>
class run_finally {
public:
    explicit run_finally(Func &&fn) : fn_(std::move(fn)) {}
    run_finally(const run_finally &) = delete;
    run_finally &operator=(const run_finally &) = delete;
    ~run_finally() { fn_(); }

private:
    Func fn_;
};

class mmap_data {
public:
    explicit mmap_data(const char *path) {
        std::ifstream file(path, std::ios::binary);
        if (!file) return;
        file.seekg(0, std::ios::end);
        const auto size = file.tellg();
        if (size <= 0) return;
        data_.resize(static_cast<size_t>(size));
        file.seekg(0, std::ios::beg);
        file.read(reinterpret_cast<char *>(data_.data()), static_cast<std::streamsize>(size));
    }

    const uint8_t *data() const noexcept { return data_.data(); }
    size_t size() const noexcept { return data_.size(); }

private:
    std::vector<uint8_t> data_;
};

class Utf8CStr {
public:
    Utf8CStr(const char *value) : value_(value ? value : "") {}
    const char *data() const noexcept { return value_.c_str(); }
    size_t size() const noexcept { return value_.size(); }

private:
    std::string value_;
};
