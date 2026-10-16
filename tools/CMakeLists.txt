file(GLOB_RECURSE tool_srcs CONFIGURE_DEPENDS
    ${CMAKE_CURRENT_SOURCE_DIR}/*.c)

foreach(src ${tool_srcs})
    get_filename_component(name ${src} NAME_WE)
    add_executable(${name} ${src})
    target_compile_options(${name} PRIVATE -O2)
endforeach()
