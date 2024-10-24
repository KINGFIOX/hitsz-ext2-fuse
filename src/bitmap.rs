use super::*;
use block_cache::BlockCacheManager;
use block_device::BlockDevice;
use logger::LogManager;

use std::sync::{Arc, Mutex};

#[allow(unused)]
pub struct BitMap {
    start: usize,
    blocks: usize, // # of blocks, sum, including the meta_data blocks
    blk_dev: Arc<dyn BlockDevice>,
}

impl BitMap {
    #[allow(unused)]
    pub fn new(start_block_no: usize, blocks: usize, blk_dev: Arc<dyn BlockDevice>) -> Self {
        Self {
            start: start_block_no,
            blocks,
            blk_dev,
        }
    }

    pub fn blk_dev(&self) -> Arc<dyn BlockDevice> {
        self.blk_dev.clone()
    }

    /// # warning
    /// should be enveloped by begin_op() and end_op()
    pub fn alloc(
        &self,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        log_mgr: Arc<Mutex<LogManager>>,
    ) -> Option<usize> {
        // TODO 这里可以做的更快一些, 不必每次都 get_block_cache
        for bno in 0..self.blocks {
            let bi = bno / BPB; // segment
            let bj = bno % BPB; // offset
            let byte = bj / 8; // 第几个 byte
            let bit = bj % 8;
            let mask = 1 << bit;
            let block_cache = blk_cch_mgr
                .lock()
                .unwrap()
                .get_block_cache(self.start + bi, self.blk_dev.clone());
            let mut guard = block_cache.lock().unwrap();
            let cache = guard.cache_mut();
            if cache[byte] & mask == 0 {
                cache[byte] |= mask;
                log_mgr
                    .lock()
                    .unwrap()
                    .write(self.start + bi, block_cache.clone()); // 这个要与上面 get_block_cache 保持一致

                // brelse(bp);
                return Some(bno);
            }
        }
        None
    }

    /// # warning
    /// should be enveloped by begin_op() and end_op()
    pub fn dealloc(
        &self,
        bno: usize,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        log_mgr: Arc<Mutex<LogManager>>,
    ) {
        let bi = bno / BPB;
        let bj = bno % BPB;
        let block_cache = blk_cch_mgr
            .lock()
            .unwrap()
            .get_block_cache(self.start + bi, self.blk_dev.clone());
        let mut guard = block_cache.lock().unwrap();
        let byte = bj / 8;
        let bit = bj % 8;
        let mask = 1 << bit;
        let cache = guard.cache_mut();
        assert!(cache[byte] & mask != 0);
        if cache[byte] & mask != 0 {
            cache[byte] &= !mask;
            log_mgr
                .lock()
                .unwrap()
                .write(self.start + bi, block_cache.clone());
        }
    }
}
