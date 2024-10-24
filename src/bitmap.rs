use super::*;
use block_cache::BlockCacheManager;
use block_device::BlockDevice;
use logger::LogManager;

use std::sync::{Arc, Mutex};

#[allow(unused)]
pub struct Bitmap {
    start_block_no: usize,
    blocks: usize,
    blk_dev: Arc<dyn BlockDevice>,
}

impl Bitmap {
    #[allow(unused)]
    /// should be enveloped by begin_op() and end_op()
    pub fn alloc(
        &self,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        log_mgr: Arc<Mutex<LogManager>>,
    ) -> Option<usize> {
        for bno in 0..self.blocks {
            let block_cache = blk_cch_mgr
                .lock()
                .unwrap()
                .get_block_cache(self.start_block_no + bno, self.blk_dev.clone());
            let mut guard = block_cache.lock().unwrap();
            let byte = bno / 8;
            let bit = bno % 8;
            let mask = 1 << bit;
            let cache = guard.cache_mut();
            if cache[byte] & mask == 0 {
                cache[byte] |= mask;
                log_mgr
                    .lock()
                    .unwrap()
                    .log_write(self.start_block_no + bno, block_cache.clone());
                return Some(bno);
            }
        }
        None
    }

    #[allow(unused)]
    /// should be enveloped by begin_op() and end_op()
    pub fn dealloc(
        &self,
        bno: usize,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        log_mgr: Arc<Mutex<LogManager>>,
    ) {
        let block_cache = blk_cch_mgr
            .lock()
            .unwrap()
            .get_block_cache(self.start_block_no + bno, self.blk_dev.clone());
        let mut guard = block_cache.lock().unwrap();
        let byte = bno / 8;
        let bit = bno % 8;
        let mask = 1 << bit;
        let cache = guard.cache_mut();
        assert!(cache[byte] & mask != 0);
        if cache[byte] & mask != 0 {
            cache[byte] &= !mask;
            log_mgr
                .lock()
                .unwrap()
                .log_write(self.start_block_no + bno, block_cache.clone());
        }
    }
}
