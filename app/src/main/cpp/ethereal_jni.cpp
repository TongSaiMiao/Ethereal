/* SPDX-License-Identifier: GPL-2.0-or-later */
/*
 * Copyright (C) 2023 bmax121. All Rights Reserved.
 * Copyright (C) 2024 GarfieldHan. All Rights Reserved.
 * Copyright (C) 2024 1f2003d5. All Rights Reserved.
 */

#include <cstring>
#include <cerrno>
#include <array>
#include <vector>
#include <unistd.h>

#include "ethereal_jni.hpp"
#include "supercall.h"

static std::array<uint8_t, ETHEREAL_MANAGER_TOKEN_SIZE> gManagerToken{};
static bool gManagerTokenReady = false;

static const uint8_t *managerAuthToken() {
    return gManagerTokenReady
        ? gManagerToken.data()
        : nullptr;
}

static jboolean nativeSetManagerToken(JNIEnv *env, jobject /* this */, jbyteArray token) {
    gManagerToken.fill(0);
    gManagerTokenReady = false;
    if (!token || env->GetArrayLength(token) != ETHEREAL_MANAGER_TOKEN_SIZE) return JNI_FALSE;
    env->GetByteArrayRegion(token, 0, ETHEREAL_MANAGER_TOKEN_SIZE,
                            reinterpret_cast<jbyte *>(gManagerToken.data()));
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
        gManagerToken.fill(0);
        return JNI_FALSE;
    }
    uint8_t any = 0;
    for (uint8_t value : gManagerToken) any |= value;
    if (any == 0) {
        gManagerToken.fill(0);
        return JNI_FALSE;
    }
    gManagerTokenReady = true;
    return JNI_TRUE;
}

jboolean nativeReady(JNIEnv * /* env */, jobject /* this */) {
    const uint8_t *token = managerAuthToken();
    return token && sc_hello(token) == SUPERCALL_HELLO_MAGIC
        ? JNI_TRUE
        : JNI_FALSE;
}

jlong nativeSu(JNIEnv *env, jobject /* this */, jint to_uid, jstring selinux_context_jstr) {
    const auto selinux_context = JUTFString(env, selinux_context_jstr);
    struct su_profile profile{};
    profile.uid = getuid();
    profile.to_uid = (uid_t)to_uid;
    if (selinux_context) strncpy(profile.scontext, selinux_context, sizeof(profile.scontext) - 1);
    long rc = sc_su(managerAuthToken(), &profile);
    if (rc < 0) [[unlikely]] {
        LOGE("nativeSu error: %ld", rc);
    }

    return rc;
}

jint nativeSetUidExclude(JNIEnv * /* env */, jobject /* this */, jint uid, jint exclude) {
    return static_cast<int>(sc_set_module_exclude(managerAuthToken(), (uid_t) uid, exclude));
}

jint nativeGetUidExclude(JNIEnv * /* env */, jobject /* this */, jint uid) {
    return static_cast<int>(sc_get_module_exclude(managerAuthToken(), (uid_t) uid));
}

jintArray nativeSuUids(JNIEnv *env, jobject /* this */) {
    int num = static_cast<int>(sc_su_uid_nums(managerAuthToken()));

    if (num <= 0) [[unlikely]] {
        LOGW("SuperUser Count less than 1, skip allocating vector...");
        return env->NewIntArray(0);
    }

    std::vector<uid_t> uids(num);

    long n = sc_su_allow_uids(managerAuthToken(), uids.data(), num);
    if (n > 0 && n <= num) [[unlikely]] {
        std::vector<jint> java_uids((size_t)n);
        for (long i = 0; i < n; ++i) java_uids[(size_t)i] = (jint)uids[(size_t)i];
        auto array = env->NewIntArray((jsize)n);
        env->SetIntArrayRegion(array, 0, (jsize)n, java_uids.data());
        return array;
    }

    return env->NewIntArray(0);
}

jobject nativeSuProfile(JNIEnv *env, jobject /* this */, jint uid) {
    struct su_profile profile{};
    profile.uid = (uid_t) uid;
    long rc = sc_su_uid_profile(managerAuthToken(), (uid_t) uid, &profile);
    if (rc < 0) [[unlikely]] {
        LOGE("nativeSuProfile error: %ld\n", rc);
    }
    jclass cls = env->FindClass("me/ethereal/app/Natives$Profile");
    if (!cls) return nullptr;
    jmethodID constructor = env->GetMethodID(cls, "<init>", "()V");
    if (!constructor) return nullptr;
    jfieldID uidField = env->GetFieldID(cls, "uid", "I");
    jfieldID toUidField = env->GetFieldID(cls, "toUid", "I");
    jfieldID scontextFild = env->GetFieldID(cls, "scontext", "Ljava/lang/String;");
    jobject obj = env->NewObject(cls, constructor);
    if (!obj) return nullptr;
    if (uidField) env->SetIntField(obj, uidField, (int) profile.uid);
    if (toUidField) env->SetIntField(obj, toUidField, (int) profile.to_uid);
    if (scontextFild) env->SetObjectField(obj, scontextFild, env->NewStringUTF(profile.scontext));
    return obj;
}

jlong nativeControlFeature(JNIEnv *env, jobject /* this */, jstring feature_name_jstr, jint state) {
    const auto feature_name = JUTFString(env, feature_name_jstr);

    long rc = sc_control_feature(managerAuthToken(), feature_name.get(), (int)state);
    if (rc < 0) [[unlikely]] {
        LOGE("nativeControlFeature error: %ld", rc);
    }

    return rc;
}

jlong nativeGrantSu(JNIEnv *env, jobject /* this */, jint uid, jint to_uid, jstring selinux_context_jstr) {
    const auto selinux_context = JUTFString(env, selinux_context_jstr);
    struct su_profile profile{};
    profile.uid = uid;
    profile.to_uid = to_uid;
    if (selinux_context) strncpy(profile.scontext, selinux_context, sizeof(profile.scontext) - 1);
    return sc_su_grant_uid(managerAuthToken(), &profile);
}

jlong nativeRevokeSu(JNIEnv * /* env */, jobject /* this */, jint uid) {
    return sc_su_revoke_uid(managerAuthToken(), (uid_t) uid);
}

jstring nativeSuPath(JNIEnv *env, jobject /* this */) {
    char buf[SU_PATH_MAX_LEN] = { '\0' };
    long rc = sc_su_get_path(managerAuthToken(), buf, sizeof(buf));
    if (rc < 0) [[unlikely]] {
        LOGE("nativeSuPath error: %ld", rc);
    }

    return env->NewStringUTF(buf);
}

jboolean nativeResetSuPath(JNIEnv *env, jobject /* this */, jstring su_path_jstr) {
    const auto su_path = JUTFString(env, su_path_jstr);

    return sc_su_reset_path(managerAuthToken(), su_path.get()) == 0;
}

JNIEXPORT jint JNI_OnLoad(JavaVM* vm, void * /*reserved*/) {
    LOGI("Enter OnLoad");

    JNIEnv* env{};
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) [[unlikely]] {
        LOGE("Get JNIEnv error!");
        return JNI_FALSE;
    }

    auto clazz = JNI_FindClass(env, "me/ethereal/app/Natives");
    if (clazz.get() == nullptr) [[unlikely]] {
        LOGE("Failed to find Natives class");
        return JNI_FALSE;
    }

    const static JNINativeMethod gMethods[] = {
        {"nativeSetManagerToken", "([B)Z", reinterpret_cast<void *>(&nativeSetManagerToken)},
        {"nativeReady", "()Z", reinterpret_cast<void *>(&nativeReady)},
        {"nativeSu", "(ILjava/lang/String;)J", reinterpret_cast<void *>(&nativeSu)},
        {"nativeSetUidExclude", "(II)I", reinterpret_cast<void *>(&nativeSetUidExclude)},
        {"nativeGetUidExclude", "(I)I", reinterpret_cast<void *>(&nativeGetUidExclude)},
        {"nativeSuUids", "()[I", reinterpret_cast<void *>(&nativeSuUids)},
        {"nativeSuProfile", "(I)Lme/ethereal/app/Natives$Profile;", reinterpret_cast<void *>(&nativeSuProfile)},
        {"nativeGrantSu", "(IILjava/lang/String;)J", reinterpret_cast<void *>(&nativeGrantSu)},
        {"nativeRevokeSu", "(I)J", reinterpret_cast<void *>(&nativeRevokeSu)},
        {"nativeSuPath", "()Ljava/lang/String;", reinterpret_cast<void *>(&nativeSuPath)},
        {"nativeResetSuPath", "(Ljava/lang/String;)Z", reinterpret_cast<void *>(&nativeResetSuPath)},
        {"nativeControlFeature", "(Ljava/lang/String;I)J", reinterpret_cast<void *>(&nativeControlFeature)},
    };

    if (JNI_RegisterNatives(env, clazz, gMethods, sizeof(gMethods) / sizeof(gMethods[0])) < 0) [[unlikely]] {
        LOGE("Failed to register native methods");
        return JNI_FALSE;
    }

    LOGI("JNI_OnLoad Done!");
    return JNI_VERSION_1_6;
}
