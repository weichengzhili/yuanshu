# 目标平台：x86_64 裸机
set(CMAKE_SYSTEM_NAME      Generic)
set(CMAKE_SYSTEM_PROCESSOR x86_64)

set(CMAKE_C_COMPILER        gcc)
set(CMAKE_CXX_COMPILER      g++)
set(CMAKE_ASM_NASM_COMPILER nasm)
set(CMAKE_LINKER            ld)
set(CMAKE_OBJCOPY           objcopy)

set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

set(XSO_COMMON_FLAGS
    -ffreestanding -fno-stack-protector -fno-pic -fno-pie
    -fno-builtin -fno-exceptions -fno-rtti
    -mno-red-zone -mno-sse -mno-sse2 -mcmodel=kernel -m64
    -Wall -Wextra -Werror -O2 -g
    CACHE STRING "" FORCE)

set(CMAKE_C_FLAGS_INIT   "${XSO_COMMON_FLAGS} -std=c11")
set(CMAKE_CXX_FLAGS_INIT "${XSO_COMMON_FLAGS} -std=c++20")
set(CMAKE_ASM_NASM_FLAGS_INIT "-f elf64")

set(CMAKE_C_STANDARD_LIBRARIES   "")
set(CMAKE_CXX_STANDARD_LIBRARIES "")
