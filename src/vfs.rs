use super::*;
use bitmap::BitMap;
use block_cache::BlockCacheManager;
use block_device::BlockDevice;
use disk::DiskInode;
use logger::LogManager;

use std::sync::{Arc, Mutex, Weak};

#[allow(unused)]
pub struct Inode {
    ino: usize,
    blk_dev: Arc<dyn BlockDevice>,
    disk_inode: DiskInode,
}

impl Inode {
    #[allow(unused)]
    pub fn new(ino: usize, blk_dev: Arc<dyn BlockDevice>) -> Self {
        // let dinode =
        todo!()
    }

    #[allow(unused)]
    pub fn itrunc(
        &mut self,
        bitmap: Arc<Mutex<BitMap>>,
        blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
        log_mgr: Arc<Mutex<LogManager>>,
    ) {
        for i in 0..NDIRECT {
            if self.disk_inode.bnos()[i] != 0 {
                bfree(
                    self.disk_inode.bnos()[i] as usize,
                    bitmap.clone(),
                    blk_cch_mgr.clone(),
                    log_mgr.clone(),
                );
                self.disk_inode.bnos_mut()[i] = 0;
            }
        }

        if self.disk_inode.bnos()[NDIRECT] != 0 {
            let indirect = blk_cch_mgr.lock().unwrap().get_block_cache(
                self.disk_inode.bnos()[NDIRECT] as usize,
                self.blk_dev.clone(),
            );
            let mut indirect_guard = indirect.lock().unwrap();
            let indirect_cache = indirect_guard.get_mut::<[u32; NINDIRECT]>(0);
            for it in indirect_cache.iter_mut() {
                if *it != 0 {
                    bfree(
                        *it as usize,
                        bitmap.clone(),
                        blk_cch_mgr.clone(),
                        log_mgr.clone(),
                    );
                    *it = 0;
                }
            }
            bfree(
                self.disk_inode.bnos()[NDIRECT] as usize,
                bitmap.clone(),
                blk_cch_mgr.clone(),
                log_mgr.clone(),
            );
            self.disk_inode.bnos_mut()[NDIRECT] = 0;
        }
    }
}

pub struct InodeManager(Vec<(usize /* blockno */, Weak<Mutex<Inode>>)>);

impl InodeManager {
    #[allow(unused)]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[allow(unused)]
    pub fn iget(&mut self, ino: usize) -> Arc<Mutex<Inode>> {
        self.0.retain(|pair| pair.1.upgrade().is_some()); // remove dead weak references
        if let Some(pair) = self.0.iter().find(|pair| pair.0 == ino) {
            pair.1.upgrade().unwrap()
        } else {
            // let inode = Arc::new(Mutex::new(Inode {
            //     ino,
            //     disk_inode: DiskInode::new(),
            // }));
            // self.0.push((ino, Arc::downgrade(&inode)));
            // inode
            todo!()
        }
    }
}

#[allow(unused)]
/// should be enveloped by begin_op() and end_op()
fn bfree(
    bno: usize,
    bitmap: Arc<Mutex<BitMap>>,
    blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
    log_mgr: Arc<Mutex<LogManager>>,
) {
    bitmap.lock().unwrap().dealloc(bno, blk_cch_mgr, log_mgr);
    // 释放的时候, 不会清空, 相反: 在 alloc 的时候才会清空
}

#[allow(unused)]
/// should be enveloped by begin_op() and end_op()
fn balloc(
    bitmap: Arc<Mutex<BitMap>>,
    blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
    log_mgr: Arc<Mutex<LogManager>>,
) -> Option<usize> {
    let bitmap_clone = bitmap.clone();
    let bitmap_guard = bitmap_clone.lock().unwrap();
    if let Some(bno) = bitmap_guard.alloc(blk_cch_mgr.clone(), log_mgr.clone()) {
        bzero(bno, blk_cch_mgr, bitmap, log_mgr);
        Some(bno)
    } else {
        None
    }
}

/// should be enveloped by begin_op() and end_op()
fn bzero(
    bno: usize,
    blk_cch_mgr: Arc<Mutex<BlockCacheManager>>,
    bitmap: Arc<Mutex<BitMap>>,
    log_mgr: Arc<Mutex<LogManager>>,
) {
    let blk_dev = bitmap.lock().unwrap().blk_dev().clone();
    // clear
    let dst = blk_cch_mgr.lock().unwrap().get_block_cache(bno, blk_dev);
    let mut dst_guard = dst.lock().unwrap();
    *dst_guard.cache_mut() = [0u8; BSIZE];
    log_mgr.lock().unwrap().log_write(bno, dst.clone());
    // brelse(dst);
}
