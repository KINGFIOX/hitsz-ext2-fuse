//! Disk layout:
//! [ boot block | super block | log_hdr + logs(30) | inode blocks(6) | free bit map(3) | data blocks(959)]
//! 1000 blocks in total

use super::*;

#[allow(unused)]
#[repr(C)]
pub struct SuperBlock {
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

impl SuperBlock {
    pub fn inode_start(&self) -> usize {
        self.inode_start as usize
    }
}

#[allow(unused)]
#[repr(i16)]
#[derive(Clone, Copy)]
pub enum FileKind {
    Invalid = 0,
    Directory = 1,
    File = 2,
    Device = 3,
}

/// inode on disk
#[allow(unused)]
#[repr(C)]
#[derive(Clone)]
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
    #[allow(unused)]
    pub fn kind(&mut self) -> &FileKind {
        &self.kind
    }

    #[allow(unused)]
    pub fn kind_mut(&mut self) -> &mut FileKind {
        &mut self.kind
    }

    #[allow(unused)]
    pub fn major(&self) -> i16 {
        self.major
    }

    #[allow(unused)]
    pub fn major_mut(&mut self) -> &mut i16 {
        &mut self.major
    }

    #[allow(unused)]
    pub fn minor(&self) -> i16 {
        self.minor
    }

    #[allow(unused)]
    pub fn minor_mut(&mut self) -> &mut i16 {
        &mut self.minor
    }

    #[allow(unused)]
    pub fn n_link(&self) -> i16 {
        self.n_link
    }

    #[allow(unused)]
    pub fn n_link_mut(&mut self) -> &mut i16 {
        &mut self.n_link
    }

    #[allow(unused)]
    pub fn size(&self) -> u32 {
        self.size
    }

    #[allow(unused)]
    pub fn size_mut(&mut self) -> &mut u32 {
        &mut self.size
    }

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
