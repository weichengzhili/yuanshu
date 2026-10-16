add_subdirectory(lib)
add_subdirectory(log)
add_subdirectory(mm)
add_subdirectory(sync)
add_subdirectory(sched)
add_subdirectory(fs)

if (XSO_NET_STACK)
    add_subdirectory(net)
endif()

add_subdirectory(driver)
add_subdirectory(syscall)
add_subdirectory(ipc)
add_subdirectory(elf)

# main.cpp + init.cpp
if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/main.cpp)
    add_library(xso_kernel_main STATIC main.cpp)
    if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/init.cpp)
        target_sources(xso_kernel_main PRIVATE init.cpp)
    endif()
    target_include_directories(xso_kernel_main PUBLIC
        ${CMAKE_SOURCE_DIR}/include
        ${CMAKE_SOURCE_DIR}/kernel
        ${CMAKE_BINARY_DIR}/include)
else()
    add_library(xso_kernel_main INTERFACE)
endif()

# 统一接口库
add_library(xso_kernel INTERFACE)
target_link_libraries(xso_kernel INTERFACE
    xso_kernel_main xso_lib xso_log xso_mm xso_sync
    xso_sched xso_fs xso_driver xso_syscall xso_ipc xso_elf)

if (XSO_NET_STACK)
    target_link_libraries(xso_kernel INTERFACE xso_net)
endif()
