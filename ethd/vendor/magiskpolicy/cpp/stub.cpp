#include <cstddef>
#include <cstdint>

struct Xperm { uint16_t low; uint16_t high; bool reset; };

extern "C" {
void *eth_policy_from_file(const char *) { return nullptr; }
void *eth_policy_from_data(const uint8_t *, size_t) { return nullptr; }
void *eth_policy_from_split() { return nullptr; }
void *eth_policy_compile_split() { return nullptr; }
void eth_policy_free(void *) {}
bool eth_policy_to_file(const void *, const char *) { return false; }
void eth_policy_allow(void *, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t) {}
void eth_policy_deny(void *, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t) {}
void eth_policy_auditallow(void *, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t) {}
void eth_policy_dontaudit(void *, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t) {}
void eth_policy_xperm(void *, int, const char *const *, size_t, const char *const *, size_t, const char *const *, size_t, const Xperm *, size_t) {}
void eth_policy_type_state(void *, bool, const char *const *, size_t) {}
void eth_policy_typeattribute(void *, const char *const *, size_t, const char *const *, size_t) {}
void eth_policy_type(void *, const char *, const char *const *, size_t) {}
void eth_policy_attribute(void *, const char *) {}
void eth_policy_type_rule(void *, int, const char *, const char *, const char *, const char *, const char *) {}
void eth_policy_genfscon(void *, const char *, const char *, const char *) {}
void eth_policy_print_rules(const void *) {}
}
