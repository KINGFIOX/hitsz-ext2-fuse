use super::*;
use blk_cch::{BlockCache, BlockCacheManager};
use blk_dev::BlockDevice;

use std::sync::{Arc, Mutex};

#[allow(unused)]
pub struct LogManager {
    start: usize,
    size: usize,
    outstanding: usize,
    committing: bool,
    table: Vec<(usize /* blockno */, Arc<Mutex<BlockCache>>)>,
}

/// log header on disk
#[allow(unused)]
#[repr(C)]
struct LogHeader {
    n: i32,
    blocks: [i32; BLOCK_SZ], //  blockno
}

impl LogManager {
    /// log_blocks(disk) -> data_blocks(disk). only called in init and commit.
    fn install_trans(&mut self, blk_cch_mgr: Arc<Mutex<BlockCacheManager>>) {
        for (i, (_, block_cache)) in self.table.iter().enumerate() {
            let mut _dst_guard = block_cache.lock().unwrap();
            let dev = _dst_guard.block_device();
            let src = blk_cch_mgr
                .lock()
                .unwrap()
                .get_block_cache(self.start + i + 1, dev); // + 1 for log header
            let mut _src_guard = src.lock().unwrap();
            BlockCache::memmove(&mut _dst_guard, &mut _src_guard); // copy log into data
            _dst_guard.write();
            // drop src
        }
    }

    /// log_mgr.table(mem) -> log_hdr(disk)
    fn write_head(
        &self,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        blk_dev: Arc<dyn BlockDevice>,
    ) {
        let block_cache = blk_cch_mgr
            .lock()
            .unwrap()
            .get_block_cache(self.start, blk_dev);
        let mut _guard_log_hdr_disk = block_cache.lock().unwrap();
        let log_hdr = _guard_log_hdr_disk.get_mut::<LogHeader>(0);
        for i in 0..log_hdr.n {
            if let Some(pair) = self.table.get(i as usize) {
                log_hdr.blocks[i as usize] = pair.0 as i32;
            } else {
                log_hdr.blocks[i as usize] = 0;
            }
        }
        _guard_log_hdr_disk.write();
    }
}

impl LogManager {
    #[allow(unused)]
    pub fn new(
        start: usize,
        size: usize,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        blk_dev: Arc<dyn BlockDevice>,
    ) -> Self {
        assert!(size_of::<LogHeader>() <= BLOCK_SZ);
        let mut log_mgr = LogManager {
            start,
            size,
            outstanding: 0,
            committing: false,
            table: Vec::new(),
        };

        // read log header from disk
        let block_cache = blk_cch_mgr
            .lock()
            .unwrap()
            .get_block_cache(start, blk_dev.clone());
        let _guard_log_hdr_disk = block_cache.lock().unwrap();
        let log_hdr_disk = _guard_log_hdr_disk.get_ref::<LogHeader>(0);
        for i in 0..log_hdr_disk.n {
            log_mgr.table.push((
                log_hdr_disk.blocks[i as usize] as usize,
                Arc::clone(&block_cache),
            ));
        }
        // drop block_cache here

        log_mgr.install_trans(blk_cch_mgr.clone());
        log_mgr.table.clear();
        log_mgr.write_head(blk_cch_mgr.clone(), blk_dev.clone());

        log_mgr
    }
}
