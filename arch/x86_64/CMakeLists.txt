xso_kernel_lib(xso_arch)

xso_link_kernel(xuanshu.elf)
target_link_libraries(xuanshu.elf PRIVATE
    -Wl,--start-group
        xso_arch
        xso_kernel
    -Wl,--end-group)

add_custom_command(TARGET xuanshu.elf POST_BUILD
    COMMAND ${CMAKE_OBJCOPY} -O binary
            $<TARGET_FILE:xuanshu.elf>
            ${CMAKE_BINARY_DIR}/bin/xuanshu.bin
    COMMENT "objcopy -> xuanshu.bin")

add_custom_target(disasm
    COMMAND objdump -d $<TARGET_FILE:xuanshu.elf> > ${CMAKE_BINARY_DIR}/xuanshu.dis
    DEPENDS xuanshu.elf)
