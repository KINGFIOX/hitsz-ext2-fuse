use super::*;
use bitmap::BitMap;
use block_cache::BlockCacheManager;
use disk::SuperBlock;
use logger::LogManager;

use std::sync::{Arc, Mutex};

pub struct XV6FS {
    bitmap: Arc<Mutex<BitMap>>,
    blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
    log_mgr: Arc<Mutex<LogManager>>,
    super_blk: SuperBlock,
}

impl XV6FS {
    pub fn bitmap(&self) -> Arc<Mutex<BitMap>> {
        self.bitmap.clone()
    }
    pub fn blk_cch_mgr(&self) -> Arc<Mutex<BlockCacheManager>> {
        self.blk_cch_mgr.clone()
    }
    pub fn log_mgr(&self) -> Arc<Mutex<LogManager>> {
        self.log_mgr.clone()
    }
    #[allow(unused)]
    pub fn super_blk(&self) -> &SuperBlock {
        &self.super_blk
    }
}
