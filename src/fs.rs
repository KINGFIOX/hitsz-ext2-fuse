use super::*;
use buf::BufCache;
use common::*;
use trans::Trans;

use core::panic;
use serde::{Deserialize, Serialize};
use std::ptr;

#[repr(C)]
#[derive(Serialize, Deserialize, Default)]
pub struct SuperBlock {
    /// Must be FSMAGIC
    pub magic: u32,
    /// Size of file system image (blocks)
    pub size: u32,
    /// Number of data blocks
    pub nblocks: u32,
    /// Number of inodes.
    pub ninodes: u32,
    /// Number of log blocks
    pub nlog: u32,
    /// Block number of first log block
    pub logstart: u32,
    /// Block number of first inode block
    pub inodestart: u32,
    /// Block number of first free map block
    pub bmapstart: u32,
}

impl SuperBlock {
    fn readsb(&mut self, bcache: &mut BufCache, dev: i32) {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        let buf = bcache.bread(dev as u32, 1);
        let dst = self as *mut _ as *mut u8;
        unsafe {
            ptr::copy(buf.data.as_ptr(), dst, BSIZE as usize);
        }
        unsafe {
            (&mut *bcache_ptr).brelse(buf);
        }
    }

    pub fn bblock(&self, bno: u32) -> u32 {
        self.bmapstart + bno / (BPB as u32)
    }

    pub fn iblock(&self, ino: u32) -> u32 {
        (self.inodestart as usize + (ino as usize) / IPB) as u32
        // #define iblock(i, sb) ((i) / ipb + sb.inodestart)
    }
}

struct FileSystem {
    bcache: BufCache,
    trans: Trans,
}

impl FileSystem {
    /// zero a block
    fn bzero(trans: &mut Trans, bcache: &mut BufCache, dev: i32, bno: i32) {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        let buf = bcache.bread(dev as u32, bno as u32);
        unsafe {
            ptr::write_bytes(buf.data.as_mut_ptr(), 0, BSIZE as usize);
        }
        trans.log_write(buf); // 记录在案
        unsafe {
            (&mut *bcache_ptr).brelse(buf);
        }
    }

    fn balloc(sb: &SuperBlock, bcache: &mut BufCache, trans: &mut Trans, dev: u32) -> u32 {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        let mut b: usize = 0;
        while b < (sb.size as usize) {
            let mut bi: usize = 0;
            let bp = bcache.bread(dev, sb.bblock(b as u32));
            while bi < BPB && b + bi < (sb.size as usize) {
                let m = 1 << (bi % 8);
                if (bp.data[bi / 8] & m) == 0 {
                    bp.data[bi / 8] |= m;
                    trans.log_write(bp);
                    unsafe {
                        (&mut *bcache_ptr).brelse(bp);
                    }
                    unsafe {
                        FileSystem::bzero(trans, &mut *bcache_ptr, dev as i32, (b + bi) as i32);
                    }
                    return (b + bi) as u32;
                }
                bi += 1;
            }
            b += BPB;
        }
        panic!("balloc: out of blocks");
    }

    fn bfree(sb: &SuperBlock, bcache: &mut BufCache, trans: &mut Trans, dev: i32, bno: u32) {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        let bp = bcache.bread(dev as u32, sb.bblock(bno));
        let bi = (bno as usize) % BPB;
        let m = 1 << (bi % 8);
        assert!(bp.data[bi / 8] & m != 0);
        bp.data[bi / 8] &= !m;
        trans.log_write(bp);
        unsafe {
            (&mut *bcache_ptr).brelse(bp);
        }
    }
}
