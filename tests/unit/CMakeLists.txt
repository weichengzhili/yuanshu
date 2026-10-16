file(GLOB tests CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/test_*.cpp)

foreach(test ${tests})
    get_filename_component(name ${test} NAME_WE)
    add_executable(${name} ${test})
    target_include_directories(${name} PRIVATE
        ${CMAKE_SOURCE_DIR}/include
        ${CMAKE_SOURCE_DIR}/kernel)
    target_compile_definitions(${name} PRIVATE XSO_HOST_TEST=1)
    target_compile_options(${name} PRIVATE -O0 -g -fsanitize=address)
    add_test(NAME ${name} COMMAND ${name})
endforeach()
