//! log, a typical use is:
//!   bp = bread(...)
//!   modify bp->data[]
//!   log_write(bp)
//!   brelse(bp)

use super::*;
use blk_cch::{BlockCache, BlockCacheManager};
use blk_dev::BlockDevice;

use std::sync::{Arc, Condvar, Mutex};

pub struct LogManager {
    start: usize,
    size: usize,
    outstanding: usize,
    /// device of log_hdr
    blk_dev: Arc<dyn BlockDevice>,
    table: Vec<(usize /* blockno */, Arc<Mutex<BlockCache>>)>,
}

/// log header on disk
#[repr(C)]
struct LogHeader {
    n: i32,
    blocks: [i32; LOGSIZE], //  blockno
}

impl LogManager {
    /// log_blocks(disk) -> data_blocks(disk). only called in init and commit.
    fn install_trans(&self, blk_cch_mgr: Arc<Mutex<BlockCacheManager>>) {
        for (i, (_, block_cache)) in self.table.iter().enumerate() {
            let mut _dst_guard = block_cache.lock().unwrap();
            let dev = _dst_guard.block_device();
            let src = blk_cch_mgr
                .lock()
                .unwrap()
                .get_block_cache(self.start + i + 1, dev); // + 1 for log header
            let mut _src_guard = src.lock().unwrap();
            BlockCache::memmove(&mut _dst_guard, &_src_guard); // copy log into data
            _dst_guard.write();
            // drop src
        }
    }

    /// log_mgr.table(mem) -> log_hdr(disk)
    fn write_head(&self, blk_cch_mgr: Arc<Mutex<BlockCacheManager>>) {
        let block_cache = blk_cch_mgr
            .lock()
            .unwrap()
            .get_block_cache(self.start, self.blk_dev.clone());
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

    /// data_blocks(disk) -> log_blocks(disk).
    fn write_log(&self, blk_cch_mgr: Arc<Mutex<BlockCacheManager>>) {
        for (i, (_, block_cache)) in self.table.iter().enumerate() {
            let mut _dst_guard = block_cache.lock().unwrap();
            let src = blk_cch_mgr
                .lock()
                .unwrap()
                .get_block_cache(self.start + i + 1, _dst_guard.block_device());
            let _src_guard = src.lock().unwrap();
            BlockCache::memmove(&mut _dst_guard, &_src_guard); // copy data into log
            _dst_guard.write();
        }
    }

    fn commit(&mut self, blk_cch_mgr: Arc<Mutex<BlockCacheManager>>) {
        if !self.table.is_empty() {
            self.write_log(blk_cch_mgr.clone());
            self.write_head(blk_cch_mgr.clone());
            self.install_trans(blk_cch_mgr.clone());
            self.table.clear();
            self.write_head(blk_cch_mgr.clone());
        }
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
        assert!(size_of::<LogHeader>() <= BSIZE);
        let mut log_mgr = LogManager {
            start,
            size,
            outstanding: 0,
            blk_dev: blk_dev.clone(),
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
        log_mgr.write_head(blk_cch_mgr.clone());

        log_mgr
    }

    #[allow(unused)]
    /// write entry to log_mgr.table(mem).
    /// WHEN COMMIT, data_blocks(disk) -> log_blocks(disk). according to log_mgr.table.
    /// and log_blocks(disk) -> data_blocks(disk).
    pub fn log_write(&mut self, blockno: usize, block: Arc<Mutex<BlockCache>>) {
        assert!(self.table.len() < self.size);
        assert!(self.outstanding > 0); // log_write outside of transaction
        if self.table.iter().any(|pair| pair.0 == blockno) {
            // log absorbtion
        } else {
            // 如果没有记录, 那么就加入一条
            self.table.push((blockno, block));
        }
    }

    #[allow(unused)]
    pub fn begin_op(this: Arc<Mutex<Self>>, cv: Arc<Condvar>) {
        let mut guard = this.lock().unwrap();
        while guard.table.len() + (guard.outstanding + 1) * MAXOPBLOCKS >= LOGSIZE {
            guard = cv.wait(guard).unwrap();
        }
        guard.outstanding += 1;
    }

    #[allow(unused)]
    pub fn end_op(
        this: Arc<Mutex<Self>>,
        cv: Arc<Condvar>,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
    ) {
        let mut guard = this.lock().unwrap();
        guard.outstanding -= 1;
        if guard.outstanding == 0 {
            guard.commit(blk_cch_mgr);
            cv.notify_all();
        }
    }
}
