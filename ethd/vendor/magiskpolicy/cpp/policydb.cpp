// Modified for Ethereal's standalone Cargo build; see ../UPSTREAM.md.
#include "include/sepolicy.hpp"

#include <sys/types.h>
#include <sys/stat.h>
#include <unistd.h>

#include <cil/cil.h>

#include <base.hpp>
#include <flags.h>

using namespace std;

#define SHALEN 64
static bool cmp_sha256(const char *a, const char *b) {
    char id_a[SHALEN] = {0};
    char id_b[SHALEN] = {0};
    if (int fd = xopen(a, O_RDONLY | O_CLOEXEC); fd >= 0) {
        xread(fd, id_a, SHALEN);
        close(fd);
    } else {
        return false;
    }

    if (int fd = xopen(b, O_RDONLY | O_CLOEXEC); fd >= 0) {
        xread(fd, id_b, SHALEN);
        close(fd);
    } else {
        return false;
    }
    LOGD("%s=[%.*s]\n", a, SHALEN, id_a);
    LOGD("%s=[%.*s]\n", b, SHALEN, id_b);
    return memcmp(id_a, id_b, SHALEN) == 0;
}

static bool check_precompiled(const char *precompiled) {
    bool ok = false;
    const char *actual_sha;
    char compiled_sha[128];

    actual_sha = PLAT_POLICY_DIR "plat_and_mapping_sepolicy.cil.sha256";
    if (access(actual_sha, R_OK) == 0) {
        ok = true;
        sprintf(compiled_sha, "%s.plat_and_mapping.sha256", precompiled);
        if (!cmp_sha256(actual_sha, compiled_sha))
            return false;
    }

    actual_sha = PLAT_POLICY_DIR "plat_sepolicy_and_mapping.sha256";
    if (access(actual_sha, R_OK) == 0) {
        ok = true;
        sprintf(compiled_sha, "%s.plat_sepolicy_and_mapping.sha256", precompiled);
        if (!cmp_sha256(actual_sha, compiled_sha))
            return false;
    }

    actual_sha = PROD_POLICY_DIR "product_sepolicy_and_mapping.sha256";
    if (access(actual_sha, R_OK) == 0) {
        ok = true;
        sprintf(compiled_sha, "%s.product_sepolicy_and_mapping.sha256", precompiled);
        if (!cmp_sha256(actual_sha, compiled_sha) != 0)
            return false;
    }

    actual_sha = SYSEXT_POLICY_DIR "system_ext_sepolicy_and_mapping.sha256";
    if (access(actual_sha, R_OK) == 0) {
        ok = true;
        sprintf(compiled_sha, "%s.system_ext_sepolicy_and_mapping.sha256", precompiled);
        if (!cmp_sha256(actual_sha, compiled_sha) != 0)
            return false;
    }

    return ok;
}

static void load_cil(struct cil_db *db, const char *file) {
    mmap_data d(file);
    cil_add_file(db, file, (const char *) d.data(), d.size());
    LOGD("cil_add [%s]\n", file);
}

static policydb_t *read_policydb(policy_file_t *pf) {
    auto db = static_cast<policydb_t *>(malloc(sizeof(policydb_t)));
    if (db == nullptr)
        return nullptr;

    if (policydb_init(db)) {
        free(db);
        return nullptr;
    }

    if (policydb_read(db, pf, 0)) {
        policydb_destroy(db);
        free(db);
        return nullptr;
    }

    return db;
}

SePolicy SePolicy::from_data(rust::Slice<const uint8_t> data) noexcept {
    LOGD("Load policy from data\n");

    policy_file_t pf;
    policy_file_init(&pf);
    pf.data = (char *) data.data();
    pf.len = data.size();
    pf.type = PF_USE_MEMORY;

    auto db = read_policydb(&pf);
    if (db == nullptr) {
        LOGE("Fail to load policy from data\n");
        return {};
    }

    return {std::make_unique<sepol_impl>(db)};
}

SePolicy SePolicy::from_file(::Utf8CStr file) noexcept {
    LOGD("Load policy from: %.*s\n", static_cast<int>(file.size()), file.data());

    policy_file_t pf;
    policy_file_init(&pf);
    auto fp = xopen_file(file.data(), "re");
    if (!fp) {
        LOGE("Fail to open policy %.*s\n", static_cast<int>(file.size()), file.data());
        return {};
    }
    pf.fp = fp.get();
    pf.type = PF_USE_STDIO;

    auto db = read_policydb(&pf);
    if (db == nullptr) {
        LOGE("Fail to load policy from %.*s\n", static_cast<int>(file.size()), file.data());
        return {};
    }

    return {std::make_unique<sepol_impl>(db)};
}

SePolicy SePolicy::compile_split() noexcept {
    char path[128], plat_ver[10] = {};
    cil_db_t *db = nullptr;
    sepol_policydb_t *pdb = nullptr;
    FILE *f;
    int policy_ver = 0;
    const char *cil_file;
#if MAGISK_DEBUG
    cil_set_log_level(CIL_INFO);
#endif
    cil_set_log_handler(+[](int lvl, const char *msg) {
        if (lvl == CIL_ERR) {
            LOGE("cil: %s", msg);
        } else if (lvl == CIL_WARN) {
            LOGW("cil: %s", msg);
        } else if (lvl == CIL_INFO) {
            LOGI("cil: %s", msg);
        } else {
            LOGD("cil: %s", msg);
        }
    });

    f = xfopen(SELINUX_VERSION, "re");
    if (f == nullptr) {
        LOGE("Fail to open %s\n", SELINUX_VERSION);
        return {};
    }
    const int policy_fields = fscanf(f, "%3d", &policy_ver);
    char trailing = '\0';
    const int policy_trailing_fields = fscanf(f, " %c", &trailing);
    fclose(f);
    if (policy_fields != 1 || policy_trailing_fields == 1) {
        LOGE("Fail to read policy version from %s\n", SELINUX_VERSION);
        return {};
    }

    if (policy_ver < POLICYDB_VERSION_MIN || policy_ver > POLICYDB_VERSION_MAX) {
        LOGE("Unsupported SELinux policy version %d\n", policy_ver);
        return {};
    }

    // Get mapping version before allocating the CIL database.
    const char *plat_ver_file = VEND_POLICY_DIR "plat_sepolicy_vers.txt";
    f = xfopen(plat_ver_file, "re");
    if (f == nullptr) {
        LOGE("Fail to open %s\n", plat_ver_file);
        return {};
    }
    const int mapping_fields = fscanf(f, "%9s", plat_ver);
    trailing = '\0';
    const int trailing_fields = fscanf(f, " %c", &trailing);
    fclose(f);
    if (mapping_fields != 1 || trailing_fields == 1) {
        LOGE("Fail to read mapping version from %s\n", plat_ver_file);
        return {};
    }

    cil_db_init(&db);
    run_finally fin([db_ptr = &db]{ cil_db_destroy(db_ptr); });
    cil_set_mls(db, 1);
    cil_set_multiple_decls(db, 1);
    cil_set_disable_neverallow(db, 1);
    cil_set_target_platform(db, SEPOL_TARGET_SELINUX);
    cil_set_attrs_expand_generated(db, 1);
    cil_set_policy_version(db, policy_ver);

    // plat
    load_cil(db, SPLIT_PLAT_CIL);

    sprintf(path, PLAT_POLICY_DIR "mapping/%s.cil", plat_ver);
    load_cil(db, path);

    sprintf(path, PLAT_POLICY_DIR "mapping/%s.compat.cil", plat_ver);
    if (access(path, R_OK) == 0)
        load_cil(db, path);

    // system_ext
    sprintf(path, SYSEXT_POLICY_DIR "mapping/%s.cil", plat_ver);
    if (access(path, R_OK) == 0)
        load_cil(db, path);

    sprintf(path, SYSEXT_POLICY_DIR "mapping/%s.compat.cil", plat_ver);
    if (access(path, R_OK) == 0)
        load_cil(db, path);

    cil_file = SYSEXT_POLICY_DIR "system_ext_sepolicy.cil";
    if (access(cil_file, R_OK) == 0)
        load_cil(db, cil_file);

    // product
    sprintf(path, PROD_POLICY_DIR "mapping/%s.cil", plat_ver);
    if (access(path, R_OK) == 0)
        load_cil(db, path);

    cil_file = PROD_POLICY_DIR "product_sepolicy.cil";
    if (access(cil_file, R_OK) == 0)
        load_cil(db, cil_file);

    // vendor
    cil_file = VEND_POLICY_DIR "nonplat_sepolicy.cil";
    if (access(cil_file, R_OK) == 0)
        load_cil(db, cil_file);

    cil_file = VEND_POLICY_DIR "plat_pub_versioned.cil";
    if (access(cil_file, R_OK) == 0)
        load_cil(db, cil_file);

    cil_file = VEND_POLICY_DIR "vendor_sepolicy.cil";
    if (access(cil_file, R_OK) == 0)
        load_cil(db, cil_file);

    // odm
    cil_file = ODM_POLICY_DIR "odm_sepolicy.cil";
    if (access(cil_file, R_OK) == 0)
        load_cil(db, cil_file);

    if (cil_compile(db))
        return {};
    if (cil_build_policydb(db, &pdb))
        return {};
    return {std::make_unique<sepol_impl>(&pdb->p)};
}

SePolicy SePolicy::from_split() noexcept {
    const char *odm_pre = ODM_POLICY_DIR "precompiled_sepolicy";
    const char *vend_pre = VEND_POLICY_DIR "precompiled_sepolicy";
    if (access(odm_pre, R_OK) == 0 && check_precompiled(odm_pre))
        return SePolicy::from_file(odm_pre);
    else if (access(vend_pre, R_OK) == 0 && check_precompiled(vend_pre))
        return SePolicy::from_file(vend_pre);
    else
        return SePolicy::compile_split();
}

sepol_impl::~sepol_impl() {
    policydb_destroy(db);
    free(db);
}

bool SePolicy::to_file(::Utf8CStr file) const noexcept {
    // No partial writes are allowed to /sys/fs/selinux/load, thus the reason why we
    // first dump everything into memory, then directly call write system call
    void *out = nullptr;
    size_t out_size = 0;
    if (policydb_to_image(nullptr, impl->db, &out, &out_size)) {
        LOGE("Fail to create policy image\n");
        free(out);
        return false;
    }

    int fd = xopen(file.data(), O_WRONLY | O_CREAT | O_CLOEXEC, 0644);
    if (fd < 0) {
        free(out);
        return false;
    }
    if (struct stat st{}; xfstat(fd, &st) == 0 && st.st_size > 0) {
        ftruncate(fd, 0);
    }
    const bool written = xwrite(fd, out, out_size) == static_cast<ssize_t>(out_size);

    close(fd);
    free(out);
    return written;
}
