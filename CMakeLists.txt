cmake_minimum_required(VERSION 3.24)

project(XuanShu
    VERSION 0.1.0
    LANGUAGES C CXX ASM_NASM)

# ---- 构建开关 ----
option(XSO_BUILD_KERNEL "Build kernel"     ON)
option(XSO_BUILD_USER   "Build user space" ON)
option(XSO_BUILD_TESTS  "Build unit tests" OFF)
option(XSO_BUILD_TOOLS  "Build host tools" ON)

set(XSO_ARCH         "x86_64" CACHE STRING "Target architecture")
set(XSO_LOG_LEVEL    "2"      CACHE STRING "Log level 0..5")
option(XSO_SMP       "SMP support"     OFF)
option(XSO_NET_STACK "Network stack"   ON)
option(XSO_NET_TCP   "TCP support"     OFF)
option(XSO_DRV_E1000 "Intel e1000 NIC" ON)
option(XSO_FS_EXT2   "ext2 filesystem" OFF)

# ---- 生成 include/config.h ----
include(cmake/kconfig.cmake)
xso_generate_config()

# ---- 公共头目录 ----
include_directories(
    ${CMAKE_SOURCE_DIR}/include
    ${CMAKE_SOURCE_DIR}/kernel
    ${CMAKE_SOURCE_DIR}/arch/${XSO_ARCH}
    ${CMAKE_BINARY_DIR}/include)

# ---- 输出路径 ----
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

# ---- 自定义函数 ----
include(cmake/functions.cmake)

# ---- 子目录 ----
add_subdirectory(kernel)
add_subdirectory(arch/${XSO_ARCH})

if (XSO_BUILD_USER)
    add_subdirectory(user)
endif()

if (XSO_BUILD_TOOLS)
    add_subdirectory(tools)
endif()

if (XSO_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# ---- 快捷目标 ----
add_custom_target(run
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/run-qemu.sh
            $<TARGET_FILE:xuanshu.elf>
    DEPENDS xuanshu.elf
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Launching QEMU")

add_custom_target(initramfs
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/mkinitramfs.sh
            ${CMAKE_BINARY_DIR}/bin/initramfs.cpio.gz
    DEPENDS xso_user_all
    COMMENT "Packing initramfs")
