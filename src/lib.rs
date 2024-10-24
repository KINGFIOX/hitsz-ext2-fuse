mod blk_cch;
mod blk_dev; // Cache for block devices
mod fs; // file system
mod logger;

pub const BLOCK_SZ: usize = 1024;
pub const MAXOPBLOCKS: usize = 10; // max # of blocks any FS op writes
pub const LOGSIZE: usize = MAXOPBLOCKS * 3; // max data blocks in on-disk log
pub const NDIRECT: usize = 12; // # of direct blocks in inode
pub const FSMAGIC: usize = 0x10203040;
pub const DIRSIZ: usize = 14;
