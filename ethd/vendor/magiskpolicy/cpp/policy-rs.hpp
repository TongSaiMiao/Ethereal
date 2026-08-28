#pragma once

#include <cstdint>
#include <memory>

#include <rust/cxx.h>
#include "policy.hpp"

struct Xperm {
    uint16_t low;
    uint16_t high;
    bool reset;
};

struct SePolicy {
    std::unique_ptr<sepol_impl> impl;

    SePolicy() noexcept = default;
    SePolicy(std::unique_ptr<sepol_impl> value) noexcept : impl(std::move(value)) {}
    SePolicy(SePolicy &&) noexcept = default;
    SePolicy &operator=(SePolicy &&) noexcept = default;
    SePolicy(const SePolicy &) = delete;
    SePolicy &operator=(const SePolicy &) = delete;
    ~SePolicy();

    void allow(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>) noexcept;
    void deny(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>) noexcept;
    void auditallow(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>) noexcept;
    void dontaudit(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>) noexcept;
    void allowxperm(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<Xperm>) noexcept;
    void auditallowxperm(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<Xperm>) noexcept;
    void dontauditxperm(rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<rust::Str>, rust::Vec<Xperm>) noexcept;
    void permissive(rust::Vec<rust::Str>) noexcept;
    void enforce(rust::Vec<rust::Str>) noexcept;
    void typeattribute(rust::Vec<rust::Str>, rust::Vec<rust::Str>) noexcept;
    void type(rust::Str, rust::Vec<rust::Str>) noexcept;
    void attribute(rust::Str) noexcept;
    void type_transition(rust::Str, rust::Str, rust::Str, rust::Str, rust::Str) noexcept;
    void type_change(rust::Str, rust::Str, rust::Str, rust::Str) noexcept;
    void type_member(rust::Str, rust::Str, rust::Str, rust::Str) noexcept;
    void genfscon(rust::Str, rust::Str, rust::Str) noexcept;
    void strip_dontaudit() noexcept;
    void print_rules() const noexcept;
    bool to_file(Utf8CStr) const noexcept;

    static SePolicy from_file(Utf8CStr) noexcept;
    static SePolicy from_split() noexcept;
    static SePolicy compile_split() noexcept;
    static SePolicy from_data(rust::Slice<const uint8_t>) noexcept;
};
