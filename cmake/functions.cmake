# 把某个目录下所有源文件编译成静态库
function(xso_kernel_lib name)
    file(GLOB_RECURSE srcs CONFIGURE_DEPENDS
        ${CMAKE_CURRENT_SOURCE_DIR}/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/*.cpp
        ${CMAKE_CURRENT_SOURCE_DIR}/*.asm)

    if (NOT srcs)
        # 目录还没有源文件，创建一个空占位库
        add_library(${name} INTERFACE)
        return()
    endif()

    add_library(${name} STATIC ${srcs})
    target_include_directories(${name} PUBLIC
        ${CMAKE_SOURCE_DIR}/include
        ${CMAKE_SOURCE_DIR}/kernel
        ${CMAKE_BINARY_DIR}/include)
    set_target_properties(${name} PROPERTIES
        POSITION_INDEPENDENT_CODE OFF
        LINKER_LANGUAGE CXX)
endfunction()

# 链接内核 ELF
function(xso_link_kernel target)
    add_executable(${target} EXCLUDE_FROM_ALL)
    target_link_options(${target} PRIVATE
        -T ${CMAKE_SOURCE_DIR}/arch/${XSO_ARCH}/linker.ld
        -nostdlib -static
        -Wl,--build-id=none
        -Wl,-z,max-page-size=4096)
    set_target_properties(${target} PROPERTIES
        OUTPUT_NAME "xuanshu"
        SUFFIX ".elf"
        LINK_DEPENDS ${CMAKE_SOURCE_DIR}/arch/${XSO_ARCH}/linker.ld)
endfunction()

# 用户态程序
function(xso_user_app name)
    add_executable(${name} ${ARGN})
    target_link_options(${name} PRIVATE
        -static -nostartfiles
        -T ${CMAKE_SOURCE_DIR}/user/user.ld)
    set_target_properties(${name} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/user/bin)
endfunction()
