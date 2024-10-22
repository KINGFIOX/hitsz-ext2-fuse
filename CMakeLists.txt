cmake_minimum_required(VERSION 3.9)
# Keep the version below in sync with the one in db.h
project(xv6fuse VERSION 1.22.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(FUSE REQUIRED)
message("FUSE_LIBRARIES ${FUSE_LIBRARIES}")

include_directories(${FUSE_INCLUDE_DIR} "./inc")

file(GLOB SOURCES "src/*.cxx")

add_executable(${PROJECT_NAME} ${SOURCES})
target_link_libraries(${PROJECT_NAME} ${FUSE_LIBRARIES})


message("FUSE_INCLUDE_DIR ${FUSE_INCLUDE_DIR}")
message("SOURCES ${SOURCES}")
