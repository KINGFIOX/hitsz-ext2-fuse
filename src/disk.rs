//! Disk layout:
//! [ boot block | super block | log_hdr + logs(30) | inode blocks(6) | free bit map(3) | data blocks(959)]
//! 1000 blocks in total

use super::*;

#[allow(unused)]
#[repr(C)]
struct SuperBlock {
    /// Must be FSMAGIC
    magic: u32,
    /// Size of file system image (blocks)
    size: u32,
    /// Number of data blocks
    n_data_block: u32,
    /// Number of inodes.
    n_inode: u32,
    /// Number of log blocks
    n_log: u32,
    /// Block number of first log block
    log_start: u32,
    /// Block number of first inode block
    inode_start: u32,
    /// Block number of first free map block
    bmapstart: u32,
}

#[allow(unused)]
#[repr(i16)]
enum FileKind {
    Invalid = 0,
    Directory = 1,
    File = 2,
    Device = 3,
}

/// inode on disk
#[allow(unused)]
#[repr(C)]
pub struct DiskInode {
    /// File type
    kind: FileKind,
    /// Major device number (T_DEVICE only)
    major: i16,
    /// Minor device number (T_DEVICE only)
    minor: i16,
    /// Number of links to inode in file system
    n_link: i16,
    /// Size of file (bytes)
    size: u32,
    /// Data block addresses
    bnos: [u32; NDIRECT + 1],
}

impl DiskInode {
    pub fn bnos(&self) -> &[u32; NDIRECT + 1] {
        &self.bnos
    }

    pub fn bnos_mut(&mut self) -> &mut [u32; NDIRECT + 1] {
        &mut self.bnos
    }
}

#[allow(unused)]
#[repr(C)]
struct DirEntry {
    inum: u16,
    name: [u8; DIRSIZ],
}
