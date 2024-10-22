#ifndef __BUF_H__
#define __BUF_H__

#include <cstdint>
#include <ctime>
#include <map>
#include <mutex>
#include <unordered_set>

#include "common.h"

struct Buffer {
  bool valid;     // has data been read from disk?
  int32_t _disk;  // does disk "own" buf?
  uint32_t _dev;  // 这两个没用
  uint32_t blockno;
  std::mutex lock;
  uint32_t refcnt;
  uint8_t data[BSIZE];
};

struct BCache {
  std::mutex lock;
  std::unordered_set<Buffer> cached;
  std::map<::timespec, Buffer> freelist;

  BCache();
  Buffer* bget(uint32_t dev, uint32_t blockno);
};

#endif  //  __BUF_H__