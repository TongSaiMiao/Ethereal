cmake_minimum_required(VERSION 3.28.0)
project("etherealjni")

find_program(CCACHE ccache)

if (CCACHE)
        set(CMAKE_CXX_COMPILER_LAUNCHER ${CCACHE})
        set(CMAKE_C_COMPILER_LAUNCHER ${CCACHE})
endif ()

find_package(cxx REQUIRED CONFIG)
link_libraries(cxx::cxx)

macro(SET_OPTION option value)
        set(${option} ${value} CACHE INTERNAL "" FORCE)
endmacro()

# NDK clang does not ship Polly; keep the remaining optimized build flags.
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -ffunction-sections -fdata-sections -fvisibility=hidden -fvisibility-inlines-hidden -O3 -flto -Wno-vla-cxx-extension")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -ffunction-sections -fdata-sections -fvisibility=hidden -fvisibility-inlines-hidden -fno-rtti -fno-exceptions -O3 -flto -Wno-vla-cxx-extension")
set(CMAKE_CXX_STANDARD 23)

add_library(${PROJECT_NAME} SHARED ethereal_jni.cpp)
add_custom_command(TARGET ${PROJECT_NAME} POST_BUILD
        COMMAND ${CMAKE_STRIP} --strip-all --remove-section=.note.gnu.build-id --remove-section=.note.android.ident $<TARGET_FILE:${PROJECT_NAME}>)

target_link_libraries(${PROJECT_NAME} PRIVATE log)
target_compile_options(${PROJECT_NAME} PRIVATE -flto)
target_link_options(${PROJECT_NAME} PRIVATE
        "-Wl,--build-id=none"
        "-Wl,-icf=all,--lto-O3"
        "-Wl,-s,-x,--gc-sections"
        "-Wl,--no-undefined"
        "-Wl,-z,max-page-size=16384"
        "-Wl,-z,common-page-size=16384")
