use super::*;
use blk_dev::BlockDevice;

use std::sync::{Arc, Mutex, Weak};

#[allow(unused)]
pub struct BlockCache {
    cache: [u8; BLOCK_SZ],
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
}

impl BlockCache {
    #[allow(unused)]
    /// Load a new BlockCache from disk.
    pub fn new(block_id: usize, block_device: Arc<dyn BlockDevice>) -> Self {
        let mut cache = [0u8; BLOCK_SZ];
        block_device.read_block(block_id, &mut cache);
        Self {
            cache,
            block_id,
            block_device,
        }
    }

    fn addr_of_offset(&self, offset: usize) -> usize {
        &self.cache[offset] as *const _ as usize
    }

    #[allow(unused)]
    pub fn get_ref<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        let addr = self.addr_of_offset(offset);
        unsafe { &*(addr as *const T) }
    }

    #[allow(unused)]
    pub fn get_mut<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        let addr = self.addr_of_offset(offset);
        unsafe { &mut *(addr as *mut T) }
    }
}

// 这里单独的保存了一份 block_id, 因为: 读取 Arc<Mutex<BlockCache>> 需要上锁, 这不好

#[allow(unused)]
pub struct BlockCacheManager(Vec<(usize /* block_id */, Weak<Mutex<BlockCache>>)>);

impl BlockCacheManager {
    #[allow(unused)]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[allow(unused)]
    pub fn get_block_cache(
        &mut self,
        block_id: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        self.0.retain(|pair| pair.1.upgrade().is_some()); // remove dead weak references

        if let Some(pair) = self.0.iter().find(|pair| pair.0 == block_id) {
            pair.1.upgrade().unwrap()
        } else {
            let block_cache = Arc::new(Mutex::new(BlockCache::new(block_id, block_device)));
            self.0.push((block_id, Arc::downgrade(&block_cache)));
            block_cache
        }
    }
}
