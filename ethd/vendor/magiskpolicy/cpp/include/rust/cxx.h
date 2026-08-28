#pragma once

#include <cstddef>
#include <vector>

namespace rust {

class Str {
public:
    constexpr Str() noexcept : data_(nullptr), size_(0) {}
    constexpr Str(const char *data, std::size_t size) noexcept : data_(data), size_(size) {}

    constexpr const char *data() const noexcept { return data_; }
    constexpr std::size_t size() const noexcept { return size_; }
    constexpr bool empty() const noexcept { return size_ == 0; }

private:
    const char *data_;
    std::size_t size_;
};

template <typename T>
class Slice {
public:
    constexpr Slice(T *data, std::size_t size) noexcept : data_(data), size_(size) {}

    constexpr T *data() const noexcept { return data_; }
    constexpr std::size_t size() const noexcept { return size_; }

private:
    T *data_;
    std::size_t size_;
};

template <typename T>
using Vec = std::vector<T>;

} // namespace rust
