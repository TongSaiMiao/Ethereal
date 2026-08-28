//
// Ethereal on 2024/6/11.
//

#ifndef ETHEREAL_JNI_HPP
#define ETHEREAL_JNI_HPP

#include <jni.h>
#include <android/log.h>
#include "jni_helper.hpp"

using namespace lsplant;

#define LOG_TAG "EtherealNative"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGW(...) __android_log_print(ANDROID_LOG_WARN, LOG_TAG, __VA_ARGS__)
#define LOGD(...) __android_log_print(ANDROID_LOG_DEBUG, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)
#define LOGV(...) __android_log_print(ANDROID_LOG_VERBOSE, LOG_TAG, __VA_ARGS__)

#endif // ETHEREAL_JNI_HPP
