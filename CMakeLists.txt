cmake_minimum_required(VERSION 3.9)
project(xv6fuse VERSION 1.22.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 14)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(PkgConfig REQUIRED)
pkg_check_modules(FUSE REQUIRED fuse)
message("FUSE_LIBRARIES ${FUSE_LIBRARIES}")

add_definitions(${FUSE_CFLAGS_OTHER})
add_compile_definitions(_FILE_OFFSET_BITS=64)
add_compile_options(-Wall -Werror)


include_directories(
    ${FUSE_INCLUDE_DIRS}
    ${CMAKE_SOURCE_DIR}/inc
)


file(GLOB SOURCES "src/*.cxx")

add_executable(${PROJECT_NAME} ${SOURCES})
target_link_libraries(${PROJECT_NAME} ${FUSE_LIBRARIES})


message("FUSE_INCLUDE_DIR ${FUSE_INCLUDE_DIR}")
message("SOURCES ${SOURCES}")
