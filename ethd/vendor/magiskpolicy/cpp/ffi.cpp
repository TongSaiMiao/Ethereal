#include "include/sepolicy.hpp"

#include <new>

SePolicy::~SePolicy() = default;

static rust::Vec<rust::Str> to_strings(const char *const *items, size_t count) {
    rust::Vec<rust::Str> result;
    result.reserve(count);
    for (size_t i = 0; i < count; ++i) {
        const char *item = items[i] ? items[i] : "";
        result.emplace_back(item, std::strlen(item));
    }
    return result;
}

static rust::Str to_string(const char *item) {
    item = item ? item : "";
    return {item, std::strlen(item)};
}

static SePolicy *boxed(SePolicy policy) {
    if (!policy.impl) return nullptr;
    return new (std::nothrow) SePolicy(std::move(policy));
}

extern "C" {

void *eth_policy_from_file(const char *path) { return boxed(SePolicy::from_file(Utf8CStr(path))); }

void *eth_policy_from_data(const uint8_t *data, size_t size) {
    return boxed(SePolicy::from_data({data, size}));
}

void *eth_policy_from_split() { return boxed(SePolicy::from_split()); }
void *eth_policy_compile_split() { return boxed(SePolicy::compile_split()); }
void eth_policy_free(void *handle) { delete static_cast<SePolicy *>(handle); }

bool eth_policy_to_file(const void *handle, const char *path) {
    return static_cast<const SePolicy *>(handle)->to_file(Utf8CStr(path));
}

void eth_policy_allow(void *h, const char *const *s, size_t sn, const char *const *t, size_t tn,
                      const char *const *c, size_t cn, const char *const *p, size_t pn) {
    static_cast<SePolicy *>(h)->allow(to_strings(s, sn), to_strings(t, tn), to_strings(c, cn), to_strings(p, pn));
}

void eth_policy_deny(void *h, const char *const *s, size_t sn, const char *const *t, size_t tn,
                     const char *const *c, size_t cn, const char *const *p, size_t pn) {
    static_cast<SePolicy *>(h)->deny(to_strings(s, sn), to_strings(t, tn), to_strings(c, cn), to_strings(p, pn));
}

void eth_policy_auditallow(void *h, const char *const *s, size_t sn, const char *const *t, size_t tn,
                           const char *const *c, size_t cn, const char *const *p, size_t pn) {
    static_cast<SePolicy *>(h)->auditallow(to_strings(s, sn), to_strings(t, tn), to_strings(c, cn), to_strings(p, pn));
}

void eth_policy_dontaudit(void *h, const char *const *s, size_t sn, const char *const *t, size_t tn,
                          const char *const *c, size_t cn, const char *const *p, size_t pn) {
    static_cast<SePolicy *>(h)->dontaudit(to_strings(s, sn), to_strings(t, tn), to_strings(c, cn), to_strings(p, pn));
}

void eth_policy_xperm(void *h, int action, const char *const *s, size_t sn, const char *const *t,
                      size_t tn, const char *const *c, size_t cn, const Xperm *p, size_t pn) {
    rust::Vec<Xperm> perms;
    if (pn != 0) perms.assign(p, p + pn);
    auto source = to_strings(s, sn);
    auto target = to_strings(t, tn);
    auto classes = to_strings(c, cn);
    if (action == 0) {
        static_cast<SePolicy *>(h)->allowxperm(std::move(source), std::move(target), std::move(classes), std::move(perms));
    } else if (action == 1) {
        static_cast<SePolicy *>(h)->auditallowxperm(std::move(source), std::move(target), std::move(classes), std::move(perms));
    } else {
        static_cast<SePolicy *>(h)->dontauditxperm(std::move(source), std::move(target), std::move(classes), std::move(perms));
    }
}

void eth_policy_type_state(void *h, bool permissive, const char *const *items, size_t count) {
    if (permissive) static_cast<SePolicy *>(h)->permissive(to_strings(items, count));
    else static_cast<SePolicy *>(h)->enforce(to_strings(items, count));
}

void eth_policy_typeattribute(void *h, const char *const *types, size_t tn,
                              const char *const *attrs, size_t an) {
    static_cast<SePolicy *>(h)->typeattribute(to_strings(types, tn), to_strings(attrs, an));
}

void eth_policy_type(void *h, const char *name, const char *const *attrs, size_t count) {
    static_cast<SePolicy *>(h)->type(to_string(name), to_strings(attrs, count));
}

void eth_policy_attribute(void *h, const char *name) {
    static_cast<SePolicy *>(h)->attribute(to_string(name));
}

void eth_policy_type_rule(void *h, int action, const char *s, const char *t, const char *c,
                          const char *d, const char *o) {
    auto *policy = static_cast<SePolicy *>(h);
    if (action == 0) policy->type_transition(to_string(s), to_string(t), to_string(c), to_string(d), to_string(o));
    else if (action == 1) policy->type_change(to_string(s), to_string(t), to_string(c), to_string(d));
    else policy->type_member(to_string(s), to_string(t), to_string(c), to_string(d));
}

void eth_policy_genfscon(void *h, const char *fs, const char *path, const char *context) {
    static_cast<SePolicy *>(h)->genfscon(to_string(fs), to_string(path), to_string(context));
}

void eth_policy_print_rules(const void *h) { static_cast<const SePolicy *>(h)->print_rules(); }

} // extern "C"
