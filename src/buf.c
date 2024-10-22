#include "buf.h"

BCache::BCache() {
  for (int i = 0; i < NBUF; i++) {
    Buffer buf;
    buf.valid = false;
    buf._disk = -1;
    buf._dev = -1;
    buf.blockno = -1;
    buf.refcnt = 0;
    cached.insert(buf);
  }
}

Buffer* BCache::bget(uint32_t dev, uint32_t blockno) {
  std::lock_guard<std::mutex> guard(lock);
  for (auto it : cached) {
    if (it->valid && it->blockno == blockno && it->_dev == dev) {
      it->refcnt++;
      return &(*it);
    }
  }
  for (auto it = cached.begin(); it != cached.end(); it++) {
    if (!it->valid) {
      it->valid = true;
      it->_dev = dev;
      it->blockno = blockno;
      it->refcnt = 1;
      return &(*it);
    }
  }
  return nullptr;
}