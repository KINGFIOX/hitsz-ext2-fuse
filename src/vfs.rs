use super::*;
use block_device::BlockDevice;
use disk::DiskInode;

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
