mod bitmap;
mod blk_cch;
mod blk_dev; // Cache for block devices
mod disk;
mod logger;
mod vfs;

pub const BSIZE: usize = 1024;
pub const MAXOPBLOCKS: usize = 10; // max # of blocks any FS op writes
pub const LOGSIZE: usize = MAXOPBLOCKS * 3; // max data blocks in on-disk log
pub const FSMAGIC: usize = 0x10203040;
pub const DIRSIZ: usize = 14;
pub const NDIRECT: usize = 12; // # of direct blocks in inode
pub const NINDIRECT: usize = BSIZE / size_of::<u32>();
pub const ROOTINO: usize = 1; // root i-number
