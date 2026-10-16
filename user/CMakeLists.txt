add_subdirectory(libc)
add_subdirectory(libcxx)
add_subdirectory(apps)

if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/rust_rt/Cargo.toml)
    add_custom_target(rust_rt ALL
        COMMAND cargo build --release
                --target x86_64-unknown-none
                --manifest-path ${CMAKE_CURRENT_SOURCE_DIR}/rust_rt/Cargo.toml
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/rust_rt
        COMMENT "Building Rust runtime")
endif()

if (EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/zig_rt/build.zig)
    add_custom_target(zig_rt ALL
        COMMAND zig build -Doptimize=ReleaseFast
        WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/zig_rt
        COMMENT "Building Zig runtime")
endif()

add_custom_target(xso_user_all ALL
    DEPENDS xso_libc xso_libcxx hello_c hello_cpp)
