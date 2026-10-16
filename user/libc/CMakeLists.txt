file(GLOB_RECURSE srcs CONFIGURE_DEPENDS
    ${CMAKE_CURRENT_SOURCE_DIR}/*.c
    ${CMAKE_CURRENT_SOURCE_DIR}/*.asm)

if (srcs)
    add_library(xso_libc STATIC ${srcs})
    target_include_directories(xso_libc PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
    target_compile_options(xso_libc PRIVATE
        -ffreestanding -fno-stack-protector -fno-pic -mno-red-zone)
else()
    add_library(xso_libc INTERFACE)
endif()
