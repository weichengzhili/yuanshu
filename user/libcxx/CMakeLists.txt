file(GLOB_RECURSE srcs CONFIGURE_DEPENDS
    ${CMAKE_CURRENT_SOURCE_DIR}/*.cpp)

if (srcs)
    add_library(xso_libcxx STATIC ${srcs})
    target_include_directories(xso_libcxx PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
    target_compile_options(xso_libcxx PRIVATE
        -ffreestanding -fno-stack-protector -fno-pic -mno-red-zone)
else()
    add_library(xso_libcxx INTERFACE)
endif()
