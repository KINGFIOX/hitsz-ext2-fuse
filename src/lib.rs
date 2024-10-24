mod blk_cch;
mod blk_dev; // Cache for block devices
mod logger;

pub const BLOCK_SZ: usize = 1024;
pub const MAXOPBLOCKS: usize = 10; // max # of blocks any FS op writes
pub const LOGSIZE: usize = MAXOPBLOCKS * 3; // max data blocks in on-disk log
