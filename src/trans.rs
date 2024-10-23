use std::borrow::BorrowMut;
use std::ptr;
use std::sync::{ Arc, Mutex };

use super::*;
use common::*;
use inode::SuperBlock;
use buf::*;

#[derive(Default)]
struct LogHeader {
    n: i32,
    block: [i32; LOGSIZE],
}

struct Trans {
    start: i32,
    size: i32,
    /// how many FS sys calls are executing.
    outstanding: i32,
    /// in commit(), please wait.
    committing: bool,
    dev: i32,
    log_hdr: LogHeader,
}

impl Trans {
    fn new(dev: i32, sb: &SuperBlock) -> Trans {
        assert!(size_of::<LogHeader>() < BSIZE);
        let trans = Trans {
            start: sb.logstart as i32,
            size: sb.nlog as i32,
            outstanding: 0,
            committing: false,
            dev,
            log_hdr: LogHeader::default(),
        };
        trans
    }

    fn read_head(&mut self, bcache: &mut BufCache) {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        let buf = bcache.bread(self.dev as u32, self.start as u32);
        let lh = unsafe { &ptr::read(buf.data.as_ptr() as *const LogHeader) };
        self.log_hdr.n = lh.n;
        for i in 0..lh.n {
            self.log_hdr.block[i as usize] = lh.block[i as usize];
        }
        unsafe { (&mut *bcache_ptr).brelse(buf) }
    }

    fn write_head(&mut self, bcache: &mut BufCache) {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        let buf = bcache.bread(self.dev as u32, self.start as u32);
        let lh = unsafe { &mut ptr::read(buf.data.as_ptr() as *mut LogHeader) };
        for i in 0..self.log_hdr.n {
            lh.block[i as usize] = self.log_hdr.block[i as usize];
        }
        BufCache::bwrite(buf);
        unsafe { (&mut *bcache_ptr).brelse(buf) }
    }

    fn install_trans(&self, bcache: &mut BufCache) {
        let bcache_ptr = bcache as *mut BufCache; // 毁灭吧
        for i in 0..self.log_hdr.n {
            let log_buf = unsafe {
                (&mut *bcache_ptr).bread(self.dev as u32, (self.start + i + 1) as u32)
            };
            let dst_buf = unsafe {
                (&mut *bcache_ptr).bread(self.dev as u32, self.log_hdr.block[i as usize] as u32)
            };
            unsafe {
                ptr::copy(log_buf.data.as_ptr(), dst_buf.data.as_mut_ptr(), BSIZE as usize);
            }
            BufCache::bwrite(dst_buf);
            dst_buf.bunpin(); // TODO 这是为什么 ?
            unsafe {
                (&mut *bcache_ptr).brelse(log_buf);
            }
            unsafe {
                (&mut *bcache_ptr).brelse(dst_buf);
            }
        }
    }

    fn recover_from_log(&mut self, bcache: &mut BufCache) {
        self.read_head(bcache);
        self.install_trans(bcache);
        self.log_hdr.n = 0;
        self.write_head(bcache);
    }

    fn begin_op(&mut self) {
        loop {
            if self.committing {
                // TODO       sleep(&log, &log.lock);
            } else if ((self.log_hdr.n + (self.outstanding + 1)) as usize) * MAXOPBLOCKS > LOGSIZE {
                // TODO       sleep(&log, &log.lock);
            } else {
                self.outstanding += 1;
                break;
            }
        }
    }
}
