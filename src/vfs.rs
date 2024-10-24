use super::*;
use disk::DiskInode;

pub struct Inode {
    ino: usize,
    disk_inode: DiskInode,
}
