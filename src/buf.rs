use super::*;
use common::*;

use std::collections::{ BTreeMap, HashSet };
use std::hash::Hash;
use std::sync::{ MutexGuard, Mutex };
use std::time::SystemTime;

struct Buf {
    /// has data been read from disk?
    valid: bool,
    /// does disk "own" buf?
    _disk: i32,
    _dev: u32,
    blockno: u32,
    refcnt: u32,
    /// LRU cache list
    data: [u8; BSIZE],
    lock: Mutex<()>,
}

impl Hash for Buf {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        todo!()
    }
}

impl Default for Buf {
    fn default() -> Self {
        Buf {
            valid: false,
            _disk: 0,
            _dev: 0,
            blockno: 0,
            refcnt: 0,
            data: [0; BSIZE],
            lock: Mutex::default(),
        }
    }
}

struct BufCache {
    cached: HashSet<Buf>,
    freelist: BTreeMap<SystemTime, Buf>,
    lock: Mutex<()>,
}

impl Default for BufCache {
    fn default() -> Self {
        let mut freelist = BTreeMap::new();
        for _ in 0..NBUF {
            let buf = Buf::default();
            freelist.insert(SystemTime::now(), buf);
        }
        BufCache {
            cached: HashSet::new(),
            freelist,
            lock: Mutex::default(),
        }
    }
}

impl BufCache {
    fn bget(&mut self, dev: u32, blockno: u32) -> (&Buf, MutexGuard<()>) {
        let bcache_lk = self.lock.lock().unwrap();
        for buf in self.cached.iter() {
            if buf._dev == dev && buf.blockno == blockno {
                buf.refcnt += 1;
                drop(bcache_lk);
                let buf_lk = buf.lock.lock().unwrap();
                return (buf, buf_lk);
            }
        }

        let Some(entry) = self.freelist.pop_first() else {
            panic!("bget: no free buffers");
        };
        let mut buf = entry.1;
        buf.valid = false;
        buf.refcnt = 1;
        buf._dev = dev;
        buf.blockno = blockno;
        self.cached.insert(buf);
        drop(bcache_lk);
        let buf_lk = buf.lock.lock().unwrap();
        return (&buf, buf_lk);
    }
}
