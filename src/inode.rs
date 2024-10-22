use super::*;
use common::*;

use serde::{ Deserialize, Serialize };
use libc::{ getgid, getuid };
use std::time::UNIX_EPOCH;

#[repr(C)]
#[derive(Serialize, Deserialize)]
pub struct SuperBlock {
    /// Must be FSMAGIC
    magic: u32,
    /// Size of file system image (blocks)
    size: u32,
    /// Number of data blocks
    nblocks: u32,
    /// Number of inodes.
    ninodes: u32,
    /// Number of log blocks
    nlog: u32,
    /// Block number of first log block
    logstart: u32,
    /// Block number of first inode block
    inodestart: u32,
    /// Block number of first free map block
    bmapstart: u32,
}

#[repr(i16)]
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FileKind {
    Directory = 1,
    File = 2,
    _Device = 3, // 这个不一定有用
    Symlink = 4,
}

impl From<FileKind> for fuser::FileType {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::File => fuser::FileType::RegularFile,
            FileKind::Directory => fuser::FileType::Directory,
            FileKind::Symlink => fuser::FileType::Symlink,
            FileKind::_Device => todo!(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Serialize, Deserialize)]
// inode on disk
pub struct DInode {
    /// File type
    kind: FileKind,
    /// Major device number (T_DEVICE only)
    _major: i16,
    /// Minor device number (T_DEVICE only)
    _minor: i16,
    /// Number of hard links to inode in file system
    nlink: i16,
    /// Size of file (bytes)
    size: u32,
    /// Data block addresses
    addrs: [u32; NDIRECT + 1 + 1],
}

#[repr(C)]
#[derive(Clone, Serialize, Deserialize)]
pub struct DirEnt {
    /// inode num
    inum: u16,
    name: [u8; DIRSIZ],
}

/// inode in memory
pub struct MInode {
    /// Inode number
    inum: u32,
    //   ref : i32,                // Reference count
    //   struct sleeplock lock;  // protects everything below here
    /// inode has been read from disk?
    valid: bool,
    /// copy of disk inode
    d_inode: DInode,
}

impl From<MInode> for fuser::FileAttr {
    fn from(value: MInode) -> Self {
        let uid = unsafe { getuid() };
        let gid = unsafe { getgid() };
        let blocks = (((value.d_inode.size as usize) + BSIZE - 1) / BSIZE) as u64;
        fuser::FileAttr {
            ino: value.inum as u64,
            size: value.d_inode.size as u64,
            blocks,
            atime: UNIX_EPOCH, // 毁灭吧
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: value.d_inode.kind.into(),
            perm: u16::MAX,
            nlink: value.d_inode.nlink as u32,
            uid,
            gid,
            rdev: 0,
            blksize: BSIZE as u32,
            flags: 0,
        }
    }
}
