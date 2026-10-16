xso_kernel_lib(xso_net)
if (XSO_NET_TCP)
    target_compile_definitions(xso_net PRIVATE CONFIG_NET_TCP=1)
endif()
