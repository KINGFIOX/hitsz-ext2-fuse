use super::*;
use blk_dev::BlockDevice;

use std::sync::{Arc, Mutex, Weak};

#[allow(unused)]
pub struct BlockCache {
    cache: [u8; BLOCK_SZ],
    blockno: usize,
    block_device: Arc<dyn BlockDevice>,
}

impl BlockCache {
    #[allow(unused)]
    pub fn blockno(&self) -> usize {
        self.blockno
    }

    #[allow(unused)]
    pub fn block_device(&self) -> Arc<dyn BlockDevice> {
        Arc::clone(&self.block_device)
    }

    pub fn memmove(dst: &mut Self, src: &mut Self) {
        dst.cache.copy_from_slice(src.cache.as_ref());
    }

    /// block(disk) -> block(mem). Load a new BlockCache from disk.
    pub fn new(blockno: usize, block_device: Arc<dyn BlockDevice>) -> Self {
        let mut cache = [0u8; BLOCK_SZ];
        block_device.read_block(blockno, &mut cache);
        Self {
            cache,
            blockno,
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

    /// block(mem) -> block(disk). Write the BlockCache to disk.
    #[allow(unused)]
    pub fn write(&self) {
        self.block_device.write_block(self.blockno, &self.cache);
    }
}

// 这里单独的保存了一份 blockno, 因为: 读取 Arc<Mutex<BlockCache>> 需要上锁, 这不好

#[allow(unused)]
pub struct BlockCacheManager(Vec<(usize /* blockno */, Weak<Mutex<BlockCache>>)>);

impl BlockCacheManager {
    #[allow(unused)]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[allow(unused)]
    pub fn get_block_cache(
        &mut self,
        blockno: usize,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        self.0.retain(|pair| pair.1.upgrade().is_some()); // remove dead weak references

        if let Some(pair) = self.0.iter().find(|pair| pair.0 == blockno) {
            pair.1.upgrade().unwrap()
        } else {
            let block_cache = Arc::new(Mutex::new(BlockCache::new(blockno, block_device)));
            self.0.push((blockno, Arc::downgrade(&block_cache)));
            block_cache
        }
    }
}