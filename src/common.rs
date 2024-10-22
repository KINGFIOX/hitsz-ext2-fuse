use super::*;
use inode::DInode;

pub const ROOTINO: u16 = 1; // root i-number

pub const FSMAGIC: u32 = 0x10203040;

/// block size
pub const BSIZE: usize = 1024;

/// direct blocks in inode
pub const NDIRECT: usize = 12;

/// number of direct blocks
pub const NINDIRECT: usize = BSIZE / size_of::<u32>();

/// max of inodes, which a file can have
pub const MAXFILE: usize = NDIRECT + NINDIRECT;

/// inodes per block
pub const IPB: usize = BSIZE / size_of::<DInode>();

/// bitmap per block
pub const BPB: usize = BSIZE * 8;

/// Directory is a file containing a sequence of dirent structures.
pub const DIRSIZ: usize = 14;

/// max # of blocks any FS op writes
pub const MAXOPBLOCKS: usize = 10;

/// max data blocks in on-disk log
pub const LOGSIZE: usize = MAXOPBLOCKS * 3;

/// size of disk block cache
pub const NBUF: usize = MAXOPBLOCKS * 3;

/// size of file system in blocks
pub const FSSIZE: usize = 1000;
