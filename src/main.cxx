#include <fuse.h>

#include <chrono>
#include <iostream>

int main(int argc, char* argv[]) {
  std::cout << "Hello, World!" << std::endl;
  auto now = std::chrono::system_clock::now();
  auto duration = now.time_since_epoch();
  std::cout << duration.count() << std::endl;
  return 0;
}