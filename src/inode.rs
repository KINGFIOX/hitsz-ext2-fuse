use super::*;
use buf::BufCache;
use common::*;
use fs::SuperBlock;
use trans::Trans;

use core::panic;
use libc::{getgid, getuid};
use serde::{Deserialize, Serialize};
use std::ptr;
use std::time::UNIX_EPOCH;

#[repr(i16)]
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FileKind {
    None = 0,
    Directory = 1,
    File = 2,
    _Device = 3, // 这个不一定有用
    Symlink = 4,
}

impl Default for FileKind {
    fn default() -> Self {
        FileKind::None
    }
}

impl From<FileKind> for fuser::FileType {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::None => unreachable!(),
            FileKind::File => fuser::FileType::RegularFile,
            FileKind::Directory => fuser::FileType::Directory,
            FileKind::Symlink => fuser::FileType::Symlink,
            FileKind::_Device => todo!(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Serialize, Deserialize, Default)]
// inode on disk
pub struct DInode {
    /// File type
    pub kind: FileKind,
    /// Major device number (T_DEVICE only)
    _major: i16,
    /// Minor device number (T_DEVICE only)
    _minor: i16,
    /// Number of hard links to inode in file system
    pub nlink: i16,
    /// Size of file (bytes)
    pub size: u32,
    /// Data block addresses
    pub addrs: [u32; NDIRECT + 1 + 1],
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
    dev: u32,
    /// Inode number
    inum: u32,
    /// Reference count
    refcnt: i32,
    //   struct sleeplock lock;  // protects everything below here
    /// inode has been read from disk?
    valid: bool,
    /// copy of disk inode
    d_inode: DInode,
}

pub struct MInodeCache {
    cache: Vec<MInode>,
}

impl MInodeCache {
    fn iget(&mut self, dev: u32, inum: u32) -> &mut MInode {
        let cache_ptr = self.cache.as_mut_ptr() as *mut Vec<MInode>;
        for inode in self.cache.iter_mut() {
            // 找, 是否在 cache 中
            if inode.refcnt > 0 && inode.inum == inum && inode.dev == dev {
                inode.refcnt += 1;
                return inode;
            }
        }
        // 不在 cache 中的话, 就分配
        let inode = MInode {
            dev,
            inum,
            refcnt: 1,
            valid: false,
            d_inode: Default::default(),
        };
        unsafe {
            (&mut *cache_ptr).push(inode);
        }
        let inode = unsafe { (&mut *cache_ptr).last_mut().unwrap() };
        inode
    }

    fn ialloc(
        &mut self,
        sb: &SuperBlock,
        trans: &mut Trans,
        bcache: &mut BufCache,
        dev: u32,
        kind: FileKind,
    ) -> &mut MInode {
        let bcache_ptr = bcache as *mut BufCache;
        for i in 1..sb.ninodes {
            let bp = bcache.bread(dev, sb.iblock(i));
            let dip_ptr = unsafe { (bp.data.as_mut_ptr() as *mut DInode).add(i as usize % IPB) };
            let dip = unsafe { &mut *dip_ptr };
            if dip.kind == FileKind::None {
                unsafe {
                    ptr::write_bytes(dip_ptr, 0, size_of::<DInode>());
                }
                dip.kind = kind;
                trans.log_write(bp);
                unsafe {
                    (&mut *bcache_ptr).brelse(bp);
                }
                return self.iget(dev, i);
            }
            unsafe {
                (&mut *bcache_ptr).brelse(bp);
            }
        }
        panic!("ialloc: no inodes")
    }

    fn iupdate(sb: &SuperBlock, trans: &mut Trans, bcache: &mut BufCache, inode: &MInode) {
        let bcache_ptr = bcache as *mut BufCache;
        let bp = bcache.bread(inode.dev, sb.iblock(inode.inum));
        let dip_ptr =
            unsafe { (bp.data.as_mut_ptr() as *mut DInode).add(inode.inum as usize % IPB) };
        let dip = unsafe { &mut *dip_ptr };
        dip.kind = inode.d_inode.kind;
        dip._major = inode.d_inode._major;
        dip._minor = inode.d_inode._minor;
        dip.nlink = inode.d_inode.nlink;
        dip.size = inode.d_inode.size;
        unsafe {
            ptr::copy(
                inode.d_inode.addrs.as_ptr(),
                dip.addrs.as_mut_ptr(),
                size_of::<[u32; NDIRECT + 1 + 1]>(),
            );
        }
        trans.log_write(bp);
        unsafe {
            (&mut *bcache_ptr).brelse(bp);
        }
    }
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
