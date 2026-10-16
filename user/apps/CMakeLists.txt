if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/hello_c.c)
    xso_user_app(hello_c hello_c.c)
    target_link_libraries(hello_c PRIVATE xso_libc)
else()
    add_custom_target(hello_c)
endif()

if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/hello_cpp.cpp)
    xso_user_app(hello_cpp hello_cpp.cpp)
    target_link_libraries(hello_cpp PRIVATE xso_libc xso_libcxx)
else()
    add_custom_target(hello_cpp)
endif()

# Rust / Zig 由各自构建系统生成
add_custom_target(hello_rust
    COMMAND ${CMAKE_COMMAND} -E echo "hello_rust (skipped, use cargo directly)")
add_custom_target(hello_zig
    COMMAND ${CMAKE_COMMAND} -E echo "hello_zig (skipped, use zig build directly)")
