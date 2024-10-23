use super::*;
use common::*;

use std::collections::HashMap;

pub struct Buffer {
    /// has data been read from disk?
    pub valid: bool,
    /// does disk "own" buf?
    pub _disk: i32,
    pub _dev: u32,
    pub blockno: u32,
    refcnt: u32,
    /// LRU cache list
    pub data: [u8; BSIZE],
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer {
            valid: false,
            _disk: 0,
            _dev: 0,
            blockno: 0,
            refcnt: 0,
            data: [0; BSIZE],
        }
    }
}

impl Buffer {
    pub fn bpin(&mut self) {
        self.refcnt += 1;
    }
    pub fn bunpin(&mut self) {
        self.refcnt -= 1; // 如果 < 0 自然会报错的
    }
}

pub struct BufCache {
    pub cached: HashMap<(u32 /* dev */, u32 /* blockno */), Buffer>,
}

impl Default for BufCache {
    fn default() -> Self {
        BufCache {
            cached: HashMap::new(),
        }
    }
}

impl BufCache {
    fn bget(&mut self, dev: u32, blockno: u32) -> &mut Buffer {
        let buf = self.cached.entry((dev, blockno)).or_default();
        // b.valid = false;
        // b._disk = 0;
        buf._dev = dev;
        buf.blockno = blockno;
        buf.refcnt += 1; // buf.refcnt = 1;
        return buf;
    }

    pub fn bread(&mut self, dev: u32, blockno: u32) -> &mut Buffer {
        let buf = self.bget(dev, blockno);
        if !buf.valid {
            // TODO read from disk
            buf.valid = true;
        }
        return buf;
    }

    #[allow(unused_variables)]
    pub fn bwrite(buf: &mut Buffer) {
        // TODO write to disk
    }

    pub fn brelse(&mut self, buf: &mut Buffer) {
        buf.refcnt -= 1;
        let refcnt = buf.refcnt;
        if refcnt == 0 {
            self.cached.remove(&(buf._dev, buf.blockno)); // drop(buf)
        }
    }
}
