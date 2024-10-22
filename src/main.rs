//! // Disk layout:
//! [ boot block | sb block | log | inode blocks | free bit map | data blocks ]

#![allow(clippy::needless_return)]
#![allow(clippy::unnecessary_cast)] // libc::S_* are u16 or u32 depending on the platform

use clap::{ Arg, ArgAction, Command };
use fuser::consts::FOPEN_DIRECT_IO;
use fuser::TimeOrNow::Now;
use fuser::{
    Filesystem,
    KernelConfig,
    MountOption,
    ReplyAttr,
    ReplyCreate,
    ReplyData,
    ReplyDirectory,
    ReplyEmpty,
    ReplyEntry,
    ReplyOpen,
    ReplyStatfs,
    ReplyWrite,
    ReplyXattr,
    Request,
    TimeOrNow,
    FUSE_ROOT_ID,
};
use libc::{ getgid, getuid };
use log::{ debug, warn };
use log::{ error, LevelFilter };
use serde::{ Deserialize, Serialize };
use std::cmp::min;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{ File, OpenOptions };
use std::io::{ BufRead, BufReader, ErrorKind, Read, Seek, SeekFrom, Write };
use std::os::raw::c_int;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;
#[cfg(target_os = "linux")]
use std::os::unix::io::IntoRawFd;
use std::path::{ Path, PathBuf };
use std::sync::atomic::{ AtomicU64, Ordering };
use std::time::{ Duration, SystemTime, UNIX_EPOCH };
use std::{ fs, io };

const ROOTINO: u16 = 1; // root i-number

const FSMAGIC: u32 = 0x10203040;

/// block size
const BSIZE: usize = 1024;

/// direct blocks in inode
const NDIRECT: usize = 12;

/// number of direct blocks
const NINDIRECT: usize = BSIZE / size_of::<u32>();

/// max of inodes, which a file can have
const MAXFILE: usize = NDIRECT + NINDIRECT;

/// inodes per block
const IPB: usize = BSIZE / size_of::<DInode>();

/// bitmap per block
const BPB: usize = BSIZE * 8;

/// Directory is a file containing a sequence of dirent structures.
const DIRSIZ: usize = 14;

/// max # of blocks any FS op writes
const MAXOPBLOCKS: usize = 10;

/// max data blocks in on-disk log
const LOGSIZE: usize = MAXOPBLOCKS * 3;

/// size of file system in blocks
const FSSIZE: usize = 1000;

#[repr(C)]
#[derive(Serialize, Deserialize)]
struct SuperBlock {
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
enum FileKind {
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
struct DInode {
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
struct DirEnt {
    /// inode num
    inum: u16,
    name: [u8; DIRSIZ],
}

/// inode in memory
struct MInode {
    /// Device number
    _dev: u32,
    /// Inode number
    inum: u32,
    //   ref : i32,                // Reference count
    //   struct sleeplock lock;  // protects everything below here
    /// inode has been read from disk?
    valid: bool,
    /// copy of disk inode
    kind: FileKind,
    _major: i16,
    _minor: i16,
    nlink: i16,
    size: u32,
    addrs: [u32; NDIRECT + 1],
}

impl From<MInode> for fuser::FileAttr {
    fn from(value: MInode) -> Self {
        let uid = unsafe { getuid() };
        let gid = unsafe { getgid() };
        let blocks = (((value.size as usize) + BSIZE - 1) / BSIZE) as u64;
        fuser::FileAttr {
            ino: value.inum as u64,
            size: value.size as u64,
            blocks,
            atime: UNIX_EPOCH, // 毁灭吧
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: value.kind.into(),
            perm: u16::MAX,
            nlink: value.nlink as u32,
            uid,
            gid,
            rdev: 0,
            blksize: BSIZE as u32,
            flags: 0,
        }
    }
}

// Stores inode metadata data in "$data_dir/inodes" and file contents in "$data_dir/contents"
// Directory data is stored in the file's contents, as a serialized DirectoryDescriptor
struct SimpleFS {
    data_dir: String,
    next_file_handle: AtomicU64,
    direct_io: bool,
    suid_support: bool,
}

impl SimpleFS {
    fn new(
        data_dir: String,
        direct_io: bool,
        #[allow(unused_variables)] suid_support: bool
    ) -> SimpleFS {
        SimpleFS {
            data_dir,
            next_file_handle: AtomicU64::new(1),
            direct_io,
            suid_support: false,
        }
    }

    fn creation_mode(&self, mode: u32) -> u16 {
        if !self.suid_support {
            (mode & (!(libc::S_ISUID | libc::S_ISGID) as u32)) as u16
        } else {
            mode as u16
        }
    }

    fn allocate_next_inode(&self) -> Inode {
        let path = Path::new(&self.data_dir).join("superblock");
        let current_inode = if let Ok(file) = File::open(&path) {
            bincode::deserialize_from(file).unwrap()
        } else {
            fuser::FUSE_ROOT_ID
        };

        let file = OpenOptions::new().write(true).create(true).truncate(true).open(&path).unwrap();
        bincode::serialize_into(file, &(current_inode + 1)).unwrap();

        current_inode + 1
    }

    fn allocate_next_file_handle(&self, read: bool, write: bool) -> u64 {
        let mut fh = self.next_file_handle.fetch_add(1, Ordering::SeqCst);
        // Assert that we haven't run out of file handles
        assert!(fh < FILE_HANDLE_READ_BIT.min(FILE_HANDLE_WRITE_BIT));
        if read {
            fh |= FILE_HANDLE_READ_BIT;
        }
        if write {
            fh |= FILE_HANDLE_WRITE_BIT;
        }

        fh
    }

    fn check_file_handle_read(&self, file_handle: u64) -> bool {
        (file_handle & FILE_HANDLE_READ_BIT) != 0
    }

    fn check_file_handle_write(&self, file_handle: u64) -> bool {
        (file_handle & FILE_HANDLE_WRITE_BIT) != 0
    }

    fn content_path(&self, inode: Inode) -> PathBuf {
        Path::new(&self.data_dir).join("contents").join(inode.to_string())
    }

    fn get_directory_content(&self, inode: Inode) -> Result<DirectoryDescriptor, c_int> {
        let path = Path::new(&self.data_dir).join("contents").join(inode.to_string());
        if let Ok(file) = File::open(path) {
            Ok(bincode::deserialize_from(file).unwrap())
        } else {
            Err(libc::ENOENT)
        }
    }

    fn write_directory_content(&self, inode: Inode, entries: DirectoryDescriptor) {
        let path = Path::new(&self.data_dir).join("contents").join(inode.to_string());
        let file = OpenOptions::new().write(true).create(true).truncate(true).open(path).unwrap();
        bincode::serialize_into(file, &entries).unwrap();
    }

    fn get_inode(&self, inode: Inode) -> Result<InodeAttributes, c_int> {
        let path = Path::new(&self.data_dir).join("inodes").join(inode.to_string());
        if let Ok(file) = File::open(path) {
            Ok(bincode::deserialize_from(file).unwrap())
        } else {
            Err(libc::ENOENT)
        }
    }

    fn write_inode(&self, inode: &InodeAttributes) {
        let path = Path::new(&self.data_dir).join("inodes").join(inode.inode.to_string());
        let file = OpenOptions::new().write(true).create(true).truncate(true).open(path).unwrap();
        bincode::serialize_into(file, inode).unwrap();
    }

    // Check whether a file should be removed from storage. Should be called after decrementing
    // the link count, or closing a file handle
    fn gc_inode(&self, inode: &InodeAttributes) -> bool {
        if inode.hardlinks == 0 && inode.open_file_handles == 0 {
            let inode_path = Path::new(&self.data_dir).join("inodes").join(inode.inode.to_string());
            fs::remove_file(inode_path).unwrap();
            let content_path = Path::new(&self.data_dir)
                .join("contents")
                .join(inode.inode.to_string());
            fs::remove_file(content_path).unwrap();

            return true;
        }

        return false;
    }

    fn truncate(
        &self,
        inode: Inode,
        new_length: u64,
        uid: u32,
        gid: u32
    ) -> Result<InodeAttributes, c_int> {
        if new_length > MAX_FILE_SIZE {
            return Err(libc::EFBIG);
        }

        let mut attrs = self.get_inode(inode)?;

        if !check_access(attrs.uid, attrs.gid, attrs.mode, uid, gid, libc::W_OK) {
            return Err(libc::EACCES);
        }

        let path = self.content_path(inode);
        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_len(new_length).unwrap();

        attrs.size = new_length;
        attrs.last_metadata_changed = time_now();
        attrs.last_modified = time_now();

        // Clear SETUID & SETGID on truncate
        clear_suid_sgid(&mut attrs);

        self.write_inode(&attrs);

        Ok(attrs)
    }

    fn lookup_name(&self, parent: u64, name: &OsStr) -> Result<InodeAttributes, c_int> {
        let entries = self.get_directory_content(parent)?;
        if let Some((inode, _)) = entries.get(name.as_bytes()) {
            return self.get_inode(*inode);
        } else {
            return Err(libc::ENOENT);
        }
    }

    fn insert_link(
        &self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        inode: u64,
        kind: FileKind
    ) -> Result<(), c_int> {
        if self.lookup_name(parent, name).is_ok() {
            return Err(libc::EEXIST);
        }

        let mut parent_attrs = self.get_inode(parent)?;

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            return Err(libc::EACCES);
        }
        parent_attrs.last_modified = time_now();
        parent_attrs.last_metadata_changed = time_now();
        self.write_inode(&parent_attrs);

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.insert(name.as_bytes().to_vec(), (inode, kind));
        self.write_directory_content(parent, entries);

        Ok(())
    }
}

impl Filesystem for SimpleFS {
    fn init(
        &mut self,
        _req: &Request,
        #[allow(unused_variables)] config: &mut KernelConfig
    ) -> Result<(), c_int> {
        fs::create_dir_all(Path::new(&self.data_dir).join("inodes")).unwrap();
        fs::create_dir_all(Path::new(&self.data_dir).join("contents")).unwrap();
        if self.get_inode(FUSE_ROOT_ID).is_err() {
            // Initialize with empty filesystem
            let root = InodeAttributes {
                inode: FUSE_ROOT_ID,
                open_file_handles: 0,
                size: 0,
                last_accessed: time_now(),
                last_modified: time_now(),
                last_metadata_changed: time_now(),
                kind: FileKind::Directory,
                mode: 0o777, // 八进制
                hardlinks: 2, // 这里是 2, 因为: inode 本身是 1, 然后目录里面有 "." 这样的记录, 这也是 1
                uid: 0,
                gid: 0,
                xattrs: Default::default(),
            };
            self.write_inode(&root); // meta data
            let mut entries = BTreeMap::new();
            entries.insert(b".".to_vec(), (FUSE_ROOT_ID, FileKind::Directory));
            self.write_directory_content(FUSE_ROOT_ID, entries); // content
        }
        Ok(())
    }

    fn lookup(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if name.len() > (MAX_NAME_LENGTH as usize) {
            reply.error(libc::ENAMETOOLONG);
            return;
        }
        let parent_attrs = self.get_inode(parent).unwrap();
        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::X_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }

        match self.lookup_name(parent, name) {
            Ok(attrs) => reply.entry(&Duration::new(0, 0), &attrs.into(), 0),
            Err(error_code) => reply.error(error_code),
        }
    }

    fn forget(&mut self, _req: &Request, _ino: u64, _nlookup: u64) {}

    fn destroy(&mut self) {}

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        match self.get_inode(ino) {
            Ok(attrs) => reply.attr(&Duration::new(0, 0), &attrs.into()),
            Err(error_code) => reply.error(error_code),
        }
    }

    fn setattr(
        &mut self,
        req: &Request,
        inode: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr
    ) {
        let mut attrs = match self.get_inode(inode) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if let Some(mode) = mode {
            debug!("chmod() called with {:?}, {:o}", inode, mode);
            if req.uid() != 0 && req.uid() != attrs.uid {
                // 不是 root, 或者不是文件的所有者
                reply.error(libc::EPERM);
                return;
            }
            if
                req.uid() != 0 &&
                req.gid() != attrs.gid &&
                !get_groups(req.pid()).contains(&attrs.gid)
            {
                // If SGID is set and the file belongs to a group that the caller is not part of
                // then the SGID bit is suppose to be cleared during chmod
                attrs.mode = (mode & (!libc::S_ISGID as u32)) as u16;
            } else {
                attrs.mode = mode as u16;
            }
            attrs.last_metadata_changed = time_now();
            self.write_inode(&attrs);
            reply.attr(&Duration::new(0, 0), &attrs.into());
            return;
        }

        if uid.is_some() || gid.is_some() {
            debug!("chown() called with {:?} {:?} {:?}", inode, uid, gid);
            if let Some(gid) = gid {
                // Non-root users can only change gid to a group they're in
                if req.uid() != 0 && !get_groups(req.pid()).contains(&gid) {
                    reply.error(libc::EPERM);
                    return;
                }
            }
            if let Some(uid) = uid {
                if
                    req.uid() != 0 &&
                    // but no-op changes by the owner are not an error
                    !(uid == attrs.uid && req.uid() == attrs.uid)
                {
                    reply.error(libc::EPERM);
                    return;
                }
            }
            // Only owner may change the group
            if gid.is_some() && req.uid() != 0 && req.uid() != attrs.uid {
                reply.error(libc::EPERM);
                return;
            }

            if (attrs.mode & ((libc::S_IXUSR | libc::S_IXGRP | libc::S_IXOTH) as u16)) != 0 {
                // SUID & SGID are suppose to be cleared when chown'ing an executable file
                clear_suid_sgid(&mut attrs);
            }

            if let Some(uid) = uid {
                attrs.uid = uid;
                // Clear SETUID on owner change
                attrs.mode &= !libc::S_ISUID as u16;
            }
            if let Some(gid) = gid {
                attrs.gid = gid;
                // Clear SETGID unless user is root
                if req.uid() != 0 {
                    attrs.mode &= !libc::S_ISGID as u16;
                }
            }
            attrs.last_metadata_changed = time_now();
            self.write_inode(&attrs);
            reply.attr(&Duration::new(0, 0), &attrs.into());
            return;
        }

        if let Some(size) = size {
            debug!("truncate() called with {:?} {:?}", inode, size);
            if let Some(handle) = fh {
                // If the file handle is available, check access locally.
                // This is important as it preserves the semantic that a file handle opened
                // with W_OK will never fail to truncate, even if the file has been subsequently
                // chmod'ed
                if self.check_file_handle_write(handle) {
                    if let Err(error_code) = self.truncate(inode, size, 0, 0) {
                        reply.error(error_code);
                        return;
                    }
                } else {
                    reply.error(libc::EACCES);
                    return;
                }
            } else if let Err(error_code) = self.truncate(inode, size, req.uid(), req.gid()) {
                reply.error(error_code);
                return;
            }
        }

        let now = time_now();
        if let Some(atime) = atime {
            debug!("utimens() called with {:?}, atime={:?}", inode, atime);

            if attrs.uid != req.uid() && req.uid() != 0 && atime != Now {
                reply.error(libc::EPERM);
                return;
            }

            if
                attrs.uid != req.uid() &&
                !check_access(attrs.uid, attrs.gid, attrs.mode, req.uid(), req.gid(), libc::W_OK)
            {
                reply.error(libc::EACCES);
                return;
            }

            attrs.last_accessed = match atime {
                TimeOrNow::SpecificTime(time) => time_from_system_time(&time),
                Now => now,
            };
            attrs.last_metadata_changed = now;
            self.write_inode(&attrs);
        }
        if let Some(mtime) = mtime {
            debug!("utimens() called with {:?}, mtime={:?}", inode, mtime);

            if attrs.uid != req.uid() && req.uid() != 0 && mtime != Now {
                reply.error(libc::EPERM);
                return;
            }

            if
                attrs.uid != req.uid() &&
                !check_access(attrs.uid, attrs.gid, attrs.mode, req.uid(), req.gid(), libc::W_OK)
            {
                reply.error(libc::EACCES);
                return;
            }

            attrs.last_modified = match mtime {
                TimeOrNow::SpecificTime(time) => time_from_system_time(&time),
                Now => now,
            };
            attrs.last_metadata_changed = now;
            self.write_inode(&attrs);
        }

        let attrs = self.get_inode(inode).unwrap();
        reply.attr(&Duration::new(0, 0), &attrs.into());
        return;
    }

    fn readlink(&mut self, _req: &Request, inode: u64, reply: ReplyData) {
        debug!("readlink() called on {:?}", inode);
        let path = self.content_path(inode);
        if let Ok(mut file) = File::open(path) {
            let file_size = file.metadata().unwrap().len();
            let mut buffer = vec![0; file_size as usize];
            file.read_exact(&mut buffer).unwrap();
            reply.data(&buffer);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn mknod(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mut mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry
    ) {
        let file_type = mode & (libc::S_IFMT as u32);

        if
            file_type != (libc::S_IFREG as u32) &&
            file_type != (libc::S_IFLNK as u32) &&
            file_type != (libc::S_IFDIR as u32)
        {
            // TODO
            warn!(
                "mknod() implementation is incomplete. Only supports regular files, symlinks, and directories. Got {:o}",
                mode
            );
            reply.error(libc::ENOSYS);
            return;
        }

        if self.lookup_name(parent, name).is_ok() {
            reply.error(libc::EEXIST);
            return;
        }

        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }
        parent_attrs.last_modified = time_now();
        parent_attrs.last_metadata_changed = time_now();

        if req.uid() != 0 {
            mode &= !(libc::S_ISUID | libc::S_ISGID) as u32;
        }

        let inode = self.allocate_next_inode();
        let mut attrs = InodeAttributes {
            inode,
            open_file_handles: 0,
            size: 0,
            last_accessed: time_now(),
            last_modified: time_now(),
            last_metadata_changed: time_now(),
            kind: as_file_kind(mode),
            mode: self.creation_mode(mode),
            hardlinks: 1,
            uid: req.uid(),
            gid: creation_gid(&parent_attrs, req.gid()),
            xattrs: Default::default(),
        };
        File::create(self.content_path(inode)).unwrap();

        if as_file_kind(mode) == FileKind::Directory {
            let mut entries = BTreeMap::new();
            entries.insert(b".".to_vec(), (inode, FileKind::Directory));
            attrs.hardlinks += 1;
            entries.insert(b"..".to_vec(), (parent, FileKind::Directory));
            parent_attrs.hardlinks += 1;
            self.write_directory_content(inode, entries);
        }
        self.write_inode(&attrs);
        self.write_inode(&parent_attrs);

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.insert(name.as_bytes().to_vec(), (inode, attrs.kind));
        self.write_directory_content(parent, entries);

        // TODO: implement flags
        reply.entry(&Duration::new(0, 0), &attrs.into(), 0);
    }

    fn mkdir(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mut mode: u32,
        _umask: u32,
        reply: ReplyEntry
    ) {
        debug!("mkdir() called with {:?} {:?} {:o}", parent, name, mode);
        if self.lookup_name(parent, name).is_ok() {
            reply.error(libc::EEXIST);
            return;
        }

        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }
        parent_attrs.last_modified = time_now();
        parent_attrs.last_metadata_changed = time_now();
        parent_attrs.hardlinks += 1;
        self.write_inode(&parent_attrs);

        if req.uid() != 0 {
            mode &= !(libc::S_ISUID | libc::S_ISGID) as u32;
        }
        if (parent_attrs.mode & (libc::S_ISGID as u16)) != 0 {
            mode |= libc::S_ISGID as u32;
        }

        let inode = self.allocate_next_inode();
        let attrs = InodeAttributes {
            inode,
            open_file_handles: 0,
            size: BLOCK_SIZE,
            last_accessed: time_now(),
            last_modified: time_now(),
            last_metadata_changed: time_now(),
            kind: FileKind::Directory,
            mode: self.creation_mode(mode),
            hardlinks: 2, // Directories start with link count of 2, since they have a self link
            uid: req.uid(),
            gid: creation_gid(&parent_attrs, req.gid()),
            xattrs: Default::default(),
        };
        self.write_inode(&attrs);

        let mut entries = BTreeMap::new();
        entries.insert(b".".to_vec(), (inode, FileKind::Directory));
        entries.insert(b"..".to_vec(), (parent, FileKind::Directory));
        self.write_directory_content(inode, entries);

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.insert(name.as_bytes().to_vec(), (inode, FileKind::Directory));
        self.write_directory_content(parent, entries);

        reply.entry(&Duration::new(0, 0), &attrs.into(), 0);
    }

    fn unlink(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!("unlink() called with {:?} {:?}", parent, name);
        let mut attrs = match self.lookup_name(parent, name) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }

        let uid = req.uid();
        // "Sticky bit" handling
        if
            (parent_attrs.mode & (libc::S_ISVTX as u16)) != 0 &&
            uid != 0 &&
            uid != parent_attrs.uid &&
            uid != attrs.uid
        {
            reply.error(libc::EACCES);
            return;
        }

        if attrs.kind == FileKind::Directory {
            parent_attrs.hardlinks -= 1;
        }

        parent_attrs.last_metadata_changed = time_now();
        parent_attrs.last_modified = time_now();
        self.write_inode(&parent_attrs);

        attrs.hardlinks -= 1;
        attrs.last_metadata_changed = time_now();
        self.write_inode(&attrs);
        self.gc_inode(&attrs);

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.remove(name.as_bytes());
        self.write_directory_content(parent, entries);

        reply.ok();
    }

    fn rmdir(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        debug!("rmdir() called with {:?} {:?}", parent, name);
        let mut attrs = match self.lookup_name(parent, name) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        // Directories always have a self and parent link
        if self.get_directory_content(attrs.inode).unwrap().len() > 2 {
            reply.error(libc::ENOTEMPTY);
            return;
        }
        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }

        // "Sticky bit" handling
        if
            (parent_attrs.mode & (libc::S_ISVTX as u16)) != 0 &&
            req.uid() != 0 &&
            req.uid() != parent_attrs.uid &&
            req.uid() != attrs.uid
        {
            reply.error(libc::EACCES);
            return;
        }

        parent_attrs.last_metadata_changed = time_now();
        parent_attrs.last_modified = time_now();
        self.write_inode(&parent_attrs);

        attrs.hardlinks = 0;
        attrs.last_metadata_changed = time_now();
        self.write_inode(&attrs);
        self.gc_inode(&attrs);

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.remove(name.as_bytes());
        self.write_directory_content(parent, entries);

        reply.ok();
    }

    fn symlink(
        &mut self,
        req: &Request,
        parent: u64,
        link_name: &OsStr,
        target: &Path,
        reply: ReplyEntry
    ) {
        debug!("symlink() called with {:?} {:?} {:?}", parent, link_name, target);
        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }
        parent_attrs.last_modified = time_now();
        parent_attrs.last_metadata_changed = time_now();
        self.write_inode(&parent_attrs);

        let inode = self.allocate_next_inode();
        let attrs = InodeAttributes {
            inode,
            open_file_handles: 0,
            size: target.as_os_str().as_bytes().len() as u64,
            last_accessed: time_now(),
            last_modified: time_now(),
            last_metadata_changed: time_now(),
            kind: FileKind::Symlink,
            mode: 0o777,
            hardlinks: 1,
            uid: req.uid(),
            gid: creation_gid(&parent_attrs, req.gid()),
            xattrs: Default::default(),
        };

        if let Err(error_code) = self.insert_link(req, parent, link_name, inode, FileKind::Symlink) {
            reply.error(error_code);
            return;
        }
        self.write_inode(&attrs);

        let path = self.content_path(inode);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.write_all(target.as_os_str().as_bytes()).unwrap();

        reply.entry(&Duration::new(0, 0), &attrs.into(), 0);
    }

    /// FIXME 这个函数有点问题, 注意一下
    fn rename(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        new_parent: u64,
        new_name: &OsStr,
        flags: u32,
        reply: ReplyEmpty
    ) {
        let mut inode_attrs = match self.lookup_name(parent, name) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }

        // "Sticky bit" handling
        if
            (parent_attrs.mode & (libc::S_ISVTX as u16)) != 0 &&
            req.uid() != 0 &&
            req.uid() != parent_attrs.uid &&
            req.uid() != inode_attrs.uid
        {
            reply.error(libc::EACCES);
            return;
        }

        let mut new_parent_attrs = match self.get_inode(new_parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                new_parent_attrs.uid,
                new_parent_attrs.gid,
                new_parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }

        // "Sticky bit" handling in new_parent
        if (new_parent_attrs.mode & (libc::S_ISVTX as u16)) != 0 {
            if let Ok(existing_attrs) = self.lookup_name(new_parent, new_name) {
                if
                    req.uid() != 0 &&
                    req.uid() != new_parent_attrs.uid &&
                    req.uid() != existing_attrs.uid
                {
                    reply.error(libc::EACCES);
                    return;
                }
            }
        }

        #[cfg(target_os = "linux")]
        if (flags & (libc::RENAME_EXCHANGE as u32)) != 0 {
            let mut new_inode_attrs = match self.lookup_name(new_parent, new_name) {
                Ok(attrs) => attrs,
                Err(error_code) => {
                    reply.error(error_code);
                    return;
                }
            };

            let mut entries = self.get_directory_content(new_parent).unwrap();
            entries.insert(new_name.as_bytes().to_vec(), (inode_attrs.inode, inode_attrs.kind));
            self.write_directory_content(new_parent, entries);

            let mut entries = self.get_directory_content(parent).unwrap();
            // entries.insert(name.as_bytes().to_vec(), (new_inode_attrs.inode, new_inode_attrs.kind));
            entries.remove(name.as_bytes());
            self.write_directory_content(parent, entries);

            inode_attrs.last_metadata_changed = time_now();
            self.write_inode(&inode_attrs);

            new_inode_attrs.last_metadata_changed = time_now();
            self.write_inode(&new_inode_attrs);

            if inode_attrs.kind == FileKind::Directory {
                let mut entries = self.get_directory_content(inode_attrs.inode).unwrap();
                entries.insert(b"..".to_vec(), (new_parent, FileKind::Directory));
                self.write_directory_content(inode_attrs.inode, entries);
                parent_attrs.hardlinks -= 1;
            }
            parent_attrs.last_metadata_changed = time_now();
            parent_attrs.last_modified = time_now();
            self.write_inode(&parent_attrs);

            if new_inode_attrs.kind == FileKind::Directory {
                let mut entries = self.get_directory_content(new_inode_attrs.inode).unwrap();
                entries.insert(b"..".to_vec(), (parent, FileKind::Directory));
                self.write_directory_content(new_inode_attrs.inode, entries);
                new_parent_attrs.hardlinks += 1;
            }
            new_parent_attrs.last_metadata_changed = time_now();
            new_parent_attrs.last_modified = time_now();
            self.write_inode(&new_parent_attrs);

            reply.ok();
            return;
        }

        // Only overwrite an existing directory if it's empty
        if let Ok(new_name_attrs) = self.lookup_name(new_parent, new_name) {
            if
                new_name_attrs.kind == FileKind::Directory &&
                self.get_directory_content(new_name_attrs.inode).unwrap().len() > 2
            {
                reply.error(libc::ENOTEMPTY);
                return;
            }
        }

        // Only move an existing directory to a new parent, if we have write access to it,
        // because that will change the ".." link in it
        if
            inode_attrs.kind == FileKind::Directory &&
            parent != new_parent &&
            !check_access(
                inode_attrs.uid,
                inode_attrs.gid,
                inode_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }

        // If target already exists decrement its hardlink count
        if let Ok(mut existing_inode_attrs) = self.lookup_name(new_parent, new_name) {
            let mut entries = self.get_directory_content(new_parent).unwrap();
            entries.remove(new_name.as_bytes());
            self.write_directory_content(new_parent, entries);

            if existing_inode_attrs.kind == FileKind::Directory {
                existing_inode_attrs.hardlinks = 0;
            } else {
                existing_inode_attrs.hardlinks -= 1;
            }
            existing_inode_attrs.last_metadata_changed = time_now();
            self.write_inode(&existing_inode_attrs);
            self.gc_inode(&existing_inode_attrs);
        }

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.remove(name.as_bytes());
        self.write_directory_content(parent, entries);

        let mut entries = self.get_directory_content(new_parent).unwrap();
        entries.insert(new_name.as_bytes().to_vec(), (inode_attrs.inode, inode_attrs.kind));
        self.write_directory_content(new_parent, entries);

        parent_attrs.last_metadata_changed = time_now();
        parent_attrs.last_modified = time_now();
        self.write_inode(&parent_attrs);
        new_parent_attrs.last_metadata_changed = time_now();
        new_parent_attrs.last_modified = time_now();
        self.write_inode(&new_parent_attrs);
        inode_attrs.last_metadata_changed = time_now();
        self.write_inode(&inode_attrs);

        if inode_attrs.kind == FileKind::Directory {
            let mut entries = self.get_directory_content(inode_attrs.inode).unwrap();
            entries.insert(b"..".to_vec(), (new_parent, FileKind::Directory));
            self.write_directory_content(inode_attrs.inode, entries);
        }

        reply.ok();
    }

    fn link(
        &mut self,
        req: &Request,
        inode: u64,
        new_parent: u64,
        new_name: &OsStr,
        reply: ReplyEntry
    ) {
        debug!("link() called for {}, {}, {:?}", inode, new_parent, new_name);
        let mut attrs = match self.get_inode(inode) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };
        if let Err(error_code) = self.insert_link(req, new_parent, new_name, inode, attrs.kind) {
            reply.error(error_code);
        } else {
            attrs.hardlinks += 1;
            attrs.last_metadata_changed = time_now();
            self.write_inode(&attrs);
            reply.entry(&Duration::new(0, 0), &attrs.into(), 0);
        }
    }

    fn open(&mut self, req: &Request, inode: u64, flags: i32, reply: ReplyOpen) {
        debug!("open() called for {:?}", inode);
        let (access_mask, read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => {
                // Behavior is undefined, but most filesystems return EACCES
                if (flags & libc::O_TRUNC) != 0 {
                    reply.error(libc::EACCES);
                    return;
                }
                if (flags & FMODE_EXEC) != 0 {
                    // Open is from internal exec syscall
                    (libc::X_OK, true, false)
                } else {
                    (libc::R_OK, true, false)
                }
            }
            libc::O_WRONLY => (libc::W_OK, false, true),
            libc::O_RDWR => (libc::R_OK | libc::W_OK, true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match self.get_inode(inode) {
            Ok(mut attr) => {
                if check_access(attr.uid, attr.gid, attr.mode, req.uid(), req.gid(), access_mask) {
                    attr.open_file_handles += 1;
                    self.write_inode(&attr);
                    let open_flags = if self.direct_io { FOPEN_DIRECT_IO } else { 0 };
                    reply.opened(self.allocate_next_file_handle(read, write), open_flags);
                } else {
                    reply.error(libc::EACCES);
                }
                return;
            }
            Err(error_code) => reply.error(error_code),
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        inode: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData
    ) {
        debug!("read() called on {:?} offset={:?} size={:?}", inode, offset, size);
        assert!(offset >= 0);
        if !self.check_file_handle_read(fh) {
            reply.error(libc::EACCES);
            return;
        }

        let path = self.content_path(inode);
        if let Ok(file) = File::open(path) {
            let file_size = file.metadata().unwrap().len();
            // Could underflow if file length is less than local_start
            let read_size = min(size, file_size.saturating_sub(offset as u64) as u32);

            let mut buffer = vec![0; read_size as usize];
            file.read_exact_at(&mut buffer, offset as u64).unwrap();
            reply.data(&buffer);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn write(
        &mut self,
        _req: &Request,
        inode: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        #[allow(unused_variables)] flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite
    ) {
        debug!("write() called with {:?} size={:?}", inode, data.len());
        assert!(offset >= 0);
        if !self.check_file_handle_write(fh) {
            reply.error(libc::EACCES);
            return;
        }

        let path = self.content_path(inode);
        if let Ok(mut file) = OpenOptions::new().write(true).open(path) {
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            file.write_all(data).unwrap();

            let mut attrs = self.get_inode(inode).unwrap();
            attrs.last_metadata_changed = time_now();
            attrs.last_modified = time_now();
            if data.len() + (offset as usize) > (attrs.size as usize) {
                attrs.size = (data.len() + (offset as usize)) as u64;
            }
            // #[cfg(feature = "abi-7-31")]
            // if flags & FUSE_WRITE_KILL_PRIV as i32 != 0 {
            //     clear_suid_sgid(&mut attrs);
            // }
            // XXX: In theory we should only need to do this when WRITE_KILL_PRIV is set for 7.31+
            // However, xfstests fail in that case
            clear_suid_sgid(&mut attrs);
            self.write_inode(&attrs);

            reply.written(data.len() as u32);
        } else {
            reply.error(libc::EBADF);
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        inode: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty
    ) {
        if let Ok(mut attrs) = self.get_inode(inode) {
            attrs.open_file_handles -= 1;
        }
        reply.ok();
    }

    fn opendir(&mut self, req: &Request, inode: u64, flags: i32, reply: ReplyOpen) {
        debug!("opendir() called on {:?}", inode);
        let (access_mask, read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => {
                // Behavior is undefined, but most filesystems return EACCES
                if (flags & libc::O_TRUNC) != 0 {
                    reply.error(libc::EACCES);
                    return;
                }
                (libc::R_OK, true, false)
            }
            libc::O_WRONLY => (libc::W_OK, false, true),
            libc::O_RDWR => (libc::R_OK | libc::W_OK, true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match self.get_inode(inode) {
            Ok(mut attr) => {
                if check_access(attr.uid, attr.gid, attr.mode, req.uid(), req.gid(), access_mask) {
                    attr.open_file_handles += 1;
                    self.write_inode(&attr);
                    let open_flags = if self.direct_io { FOPEN_DIRECT_IO } else { 0 };
                    reply.opened(self.allocate_next_file_handle(read, write), open_flags);
                } else {
                    reply.error(libc::EACCES);
                }
                return;
            }
            Err(error_code) => reply.error(error_code),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        inode: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory
    ) {
        debug!("readdir() called with {:?}", inode);
        assert!(offset >= 0);
        let entries = match self.get_directory_content(inode) {
            Ok(entries) => entries,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        for (index, entry) in entries
            .iter()
            .skip(offset as usize)
            .enumerate() {
            let (name, (inode, file_type)) = entry;

            let buffer_full: bool = reply.add(
                *inode,
                offset + (index as i64) + 1,
                (*file_type).into(),
                OsStr::from_bytes(name)
            );

            if buffer_full {
                break;
            }
        }

        reply.ok();
    }

    fn releasedir(
        &mut self,
        _req: &Request<'_>,
        inode: u64,
        _fh: u64,
        _flags: i32,
        reply: ReplyEmpty
    ) {
        if let Ok(mut attrs) = self.get_inode(inode) {
            attrs.open_file_handles -= 1;
        }
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        warn!("statfs() implementation is a stub");
        // TODO: real implementation of this
        reply.statfs(
            10_000,
            10_000,
            10_000,
            1,
            10_000,
            BLOCK_SIZE as u32,
            MAX_NAME_LENGTH,
            BLOCK_SIZE as u32
        );
    }

    fn setxattr(
        &mut self,
        request: &Request<'_>,
        inode: u64,
        key: &OsStr,
        value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty
    ) {
        if let Ok(mut attrs) = self.get_inode(inode) {
            if let Err(error) = xattr_access_check(key.as_bytes(), libc::W_OK, &attrs, request) {
                reply.error(error);
                return;
            }

            attrs.xattrs.insert(key.as_bytes().to_vec(), value.to_vec());
            attrs.last_metadata_changed = time_now();
            self.write_inode(&attrs);
            reply.ok();
        } else {
            reply.error(libc::EBADF);
        }
    }

    fn getxattr(
        &mut self,
        request: &Request<'_>,
        inode: u64,
        key: &OsStr,
        size: u32,
        reply: ReplyXattr
    ) {
        if let Ok(attrs) = self.get_inode(inode) {
            if let Err(error) = xattr_access_check(key.as_bytes(), libc::R_OK, &attrs, request) {
                reply.error(error);
                return;
            }

            if let Some(data) = attrs.xattrs.get(key.as_bytes()) {
                if size == 0 {
                    reply.size(data.len() as u32);
                } else if data.len() <= (size as usize) {
                    reply.data(data);
                } else {
                    reply.error(libc::ERANGE);
                }
            } else {
                #[cfg(target_os = "linux")]
                reply.error(libc::ENODATA);
                #[cfg(not(target_os = "linux"))]
                reply.error(libc::ENOATTR);
            }
        } else {
            reply.error(libc::EBADF);
        }
    }

    fn listxattr(&mut self, _req: &Request<'_>, inode: u64, size: u32, reply: ReplyXattr) {
        if let Ok(attrs) = self.get_inode(inode) {
            let mut bytes = vec![];
            // Convert to concatenated null-terminated strings
            for key in attrs.xattrs.keys() {
                bytes.extend(key);
                bytes.push(0);
            }
            if size == 0 {
                reply.size(bytes.len() as u32);
            } else if bytes.len() <= (size as usize) {
                reply.data(&bytes);
            } else {
                reply.error(libc::ERANGE);
            }
        } else {
            reply.error(libc::EBADF);
        }
    }

    fn removexattr(&mut self, request: &Request<'_>, inode: u64, key: &OsStr, reply: ReplyEmpty) {
        if let Ok(mut attrs) = self.get_inode(inode) {
            if let Err(error) = xattr_access_check(key.as_bytes(), libc::W_OK, &attrs, request) {
                reply.error(error);
                return;
            }

            if attrs.xattrs.remove(key.as_bytes()).is_none() {
                #[cfg(target_os = "linux")]
                reply.error(libc::ENODATA);
                #[cfg(not(target_os = "linux"))]
                reply.error(libc::ENOATTR);
                return;
            }
            attrs.last_metadata_changed = time_now();
            self.write_inode(&attrs);
            reply.ok();
        } else {
            reply.error(libc::EBADF);
        }
    }

    fn access(&mut self, req: &Request, inode: u64, mask: i32, reply: ReplyEmpty) {
        debug!("access() called with {:?} {:?}", inode, mask);
        match self.get_inode(inode) {
            Ok(attr) => {
                if check_access(attr.uid, attr.gid, attr.mode, req.uid(), req.gid(), mask) {
                    reply.ok();
                } else {
                    reply.error(libc::EACCES);
                }
            }
            Err(error_code) => reply.error(error_code),
        }
    }

    fn create(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mut mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate
    ) {
        debug!("create() called with {:?} {:?}", parent, name);
        if self.lookup_name(parent, name).is_ok() {
            reply.error(libc::EEXIST);
            return;
        }

        let (read, write) = match flags & libc::O_ACCMODE {
            libc::O_RDONLY => (true, false),
            libc::O_WRONLY => (false, true),
            libc::O_RDWR => (true, true),
            // Exactly one access mode flag must be specified
            _ => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let mut parent_attrs = match self.get_inode(parent) {
            Ok(attrs) => attrs,
            Err(error_code) => {
                reply.error(error_code);
                return;
            }
        };

        if
            !check_access(
                parent_attrs.uid,
                parent_attrs.gid,
                parent_attrs.mode,
                req.uid(),
                req.gid(),
                libc::W_OK
            )
        {
            reply.error(libc::EACCES);
            return;
        }
        parent_attrs.last_modified = time_now();
        parent_attrs.last_metadata_changed = time_now();
        self.write_inode(&parent_attrs);

        if req.uid() != 0 {
            mode &= !(libc::S_ISUID | libc::S_ISGID) as u32;
        }

        let inode = self.allocate_next_inode();
        let attrs = InodeAttributes {
            inode,
            open_file_handles: 1,
            size: 0,
            last_accessed: time_now(),
            last_modified: time_now(),
            last_metadata_changed: time_now(),
            kind: as_file_kind(mode),
            mode: self.creation_mode(mode),
            hardlinks: 1,
            uid: req.uid(),
            gid: creation_gid(&parent_attrs, req.gid()),
            xattrs: Default::default(),
        };
        self.write_inode(&attrs);
        File::create(self.content_path(inode)).unwrap();

        if as_file_kind(mode) == FileKind::Directory {
            let mut entries = BTreeMap::new();
            entries.insert(b".".to_vec(), (inode, FileKind::Directory));
            entries.insert(b"..".to_vec(), (parent, FileKind::Directory));
            self.write_directory_content(inode, entries);
        }

        let mut entries = self.get_directory_content(parent).unwrap();
        entries.insert(name.as_bytes().to_vec(), (inode, attrs.kind));
        self.write_directory_content(parent, entries);

        // TODO: implement flags
        reply.created(
            &Duration::new(0, 0),
            &attrs.into(),
            0,
            self.allocate_next_file_handle(read, write),
            0
        );
    }

    #[cfg(target_os = "linux")]
    fn fallocate(
        &mut self,
        _req: &Request<'_>,
        inode: u64,
        _fh: u64,
        offset: i64,
        length: i64,
        mode: i32,
        reply: ReplyEmpty
    ) {
        let path = self.content_path(inode);
        if let Ok(file) = OpenOptions::new().write(true).open(path) {
            unsafe {
                libc::fallocate64(file.into_raw_fd(), mode, offset, length);
            }
            if (mode & libc::FALLOC_FL_KEEP_SIZE) == 0 {
                let mut attrs = self.get_inode(inode).unwrap();
                attrs.last_metadata_changed = time_now();
                attrs.last_modified = time_now();
                if ((offset + length) as u64) > attrs.size {
                    attrs.size = (offset + length) as u64;
                }
                self.write_inode(&attrs);
            }
            reply.ok();
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn copy_file_range(
        &mut self,
        _req: &Request<'_>,
        src_inode: u64,
        src_fh: u64,
        src_offset: i64,
        dest_inode: u64,
        dest_fh: u64,
        dest_offset: i64,
        size: u64,
        _flags: u32,
        reply: ReplyWrite
    ) {
        debug!(
            "copy_file_range() called with src ({}, {}, {}) dest ({}, {}, {}) size={}",
            src_fh,
            src_inode,
            src_offset,
            dest_fh,
            dest_inode,
            dest_offset,
            size
        );
        if !self.check_file_handle_read(src_fh) {
            reply.error(libc::EACCES);
            return;
        }
        if !self.check_file_handle_write(dest_fh) {
            reply.error(libc::EACCES);
            return;
        }

        let src_path = self.content_path(src_inode);
        if let Ok(file) = File::open(src_path) {
            let file_size = file.metadata().unwrap().len();
            // Could underflow if file length is less than local_start
            let read_size = min(size, file_size.saturating_sub(src_offset as u64));

            let mut data = vec![0; read_size as usize];
            file.read_exact_at(&mut data, src_offset as u64).unwrap();

            let dest_path = self.content_path(dest_inode);
            if let Ok(mut file) = OpenOptions::new().write(true).open(dest_path) {
                file.seek(SeekFrom::Start(dest_offset as u64)).unwrap();
                file.write_all(&data).unwrap();

                let mut attrs = self.get_inode(dest_inode).unwrap();
                attrs.last_metadata_changed = time_now();
                attrs.last_modified = time_now();
                if data.len() + (dest_offset as usize) > (attrs.size as usize) {
                    attrs.size = (data.len() + (dest_offset as usize)) as u64;
                }
                self.write_inode(&attrs);

                reply.written(data.len() as u32);
            } else {
                reply.error(libc::EBADF);
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn flush(&mut self, _req: &Request<'_>, ino: u64, fh: u64, lock_owner: u64, reply: ReplyEmpty) {
        debug!(
            "[Not Implemented] flush(ino: {:#x?}, fh: {}, lock_owner: {:?})",
            ino,
            fh,
            lock_owner
        );
        reply.error(libc::ENOSYS);
    }

    fn fsync(&mut self, _req: &Request<'_>, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        debug!("[Not Implemented] fsync(ino: {:#x?}, fh: {}, datasync: {})", ino, fh, datasync);
        reply.error(libc::ENOSYS);
    }

    fn readdirplus(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        reply: fuser::ReplyDirectoryPlus
    ) {
        debug!("[Not Implemented] readdirplus(ino: {:#x?}, fh: {}, offset: {})", ino, fh, offset);
        reply.error(libc::ENOSYS);
    }

    fn fsyncdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        datasync: bool,
        reply: ReplyEmpty
    ) {
        debug!("[Not Implemented] fsyncdir(ino: {:#x?}, fh: {}, datasync: {})", ino, fh, datasync);
        reply.error(libc::ENOSYS);
    }

    fn getlk(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: i32,
        pid: u32,
        reply: fuser::ReplyLock
    ) {
        debug!(
            "[Not Implemented] getlk(ino: {:#x?}, fh: {}, lock_owner: {}, start: {}, \
            end: {}, typ: {}, pid: {})",
            ino,
            fh,
            lock_owner,
            start,
            end,
            typ,
            pid
        );
        reply.error(libc::ENOSYS);
    }

    fn setlk(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: i32,
        pid: u32,
        sleep: bool,
        reply: ReplyEmpty
    ) {
        debug!(
            "[Not Implemented] setlk(ino: {:#x?}, fh: {}, lock_owner: {}, start: {}, \
            end: {}, typ: {}, pid: {}, sleep: {})",
            ino,
            fh,
            lock_owner,
            start,
            end,
            typ,
            pid,
            sleep
        );
        reply.error(libc::ENOSYS);
    }

    fn bmap(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        blocksize: u32,
        idx: u64,
        reply: fuser::ReplyBmap
    ) {
        debug!("[Not Implemented] bmap(ino: {:#x?}, blocksize: {}, idx: {})", ino, blocksize, idx);
        reply.error(libc::ENOSYS);
    }

    fn ioctl(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        flags: u32,
        cmd: u32,
        in_data: &[u8],
        out_size: u32,
        reply: fuser::ReplyIoctl
    ) {
        debug!(
            "[Not Implemented] ioctl(ino: {:#x?}, fh: {}, flags: {}, cmd: {}, \
            in_data.len(): {}, out_size: {})",
            ino,
            fh,
            flags,
            cmd,
            in_data.len(),
            out_size
        );
        reply.error(libc::ENOSYS);
    }

    fn lseek(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        whence: i32,
        reply: fuser::ReplyLseek
    ) {
        debug!(
            "[Not Implemented] lseek(ino: {:#x?}, fh: {}, offset: {}, whence: {})",
            ino,
            fh,
            offset,
            whence
        );
        reply.error(libc::ENOSYS);
    }
}

pub fn check_access(
    file_uid: u32,
    file_gid: u32,
    file_mode: u16,
    uid: u32,
    gid: u32,
    mut access_mask: i32
) -> bool {
    // F_OK tests for existence of file
    if access_mask == libc::F_OK {
        return true;
    }
    let file_mode = i32::from(file_mode);

    // root is allowed to read & write anything
    if uid == 0 {
        // root only allowed to exec if one of the X bits is set
        access_mask &= libc::X_OK;
        access_mask -= access_mask & (file_mode >> 6);
        access_mask -= access_mask & (file_mode >> 3);
        access_mask -= access_mask & file_mode;
        return access_mask == 0;
    }

    if uid == file_uid {
        access_mask -= access_mask & (file_mode >> 6);
    } else if gid == file_gid {
        access_mask -= access_mask & (file_mode >> 3);
    } else {
        access_mask -= access_mask & file_mode;
    }

    return access_mask == 0;
}

fn as_file_kind(mut mode: u32) -> FileKind {
    mode &= libc::S_IFMT as u32;

    if mode == (libc::S_IFREG as u32) {
        return FileKind::File;
    } else if mode == (libc::S_IFLNK as u32) {
        return FileKind::Symlink;
    } else if mode == (libc::S_IFDIR as u32) {
        return FileKind::Directory;
    } else {
        unimplemented!("{}", mode);
    }
}

fn get_groups(pid: u32) -> Vec<u32> {
    #[cfg(not(target_os = "macos"))]
    {
        let path = format!("/proc/{pid}/task/{pid}/status");
        let file = File::open(path).unwrap();
        for line in BufReader::new(file).lines() {
            let line = line.unwrap();
            if line.starts_with("Groups:") {
                return line["Groups: ".len()..]
                    .split(' ')
                    .filter(|x| !x.trim().is_empty())
                    .map(|x| x.parse::<u32>().unwrap())
                    .collect();
            }
        }
    }

    vec![]
}

fn fuse_allow_other_enabled() -> io::Result<bool> {
    let file = File::open("/etc/fuse.conf")?;
    for line in BufReader::new(file).lines() {
        if line?.trim_start().starts_with("user_allow_other") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn main() {
    let matches = Command::new("Fuser")
        .author("Christopher Berner")
        .arg(
            Arg::new("data-dir") // --data-dir <DIR> 指定用于存储数据的本地目录, 默认是 /tmp/fuser
                .long("data-dir")
                .value_name("DIR")
                .default_value("/tmp/fuser")
                .help("Set local directory used to store data")
        )
        .arg(
            Arg::new("mount-point") // --mount-point <MOUNT_POINT>
                .long("mount-point")
                .value_name("MOUNT_POINT")
                .default_value("")
                .help("Act as a client, and mount FUSE at given path")
        )
        .arg(
            Arg::new("direct-io") // --direct-io
                .long("direct-io")
                .action(ArgAction::SetTrue) // 这是一个 Bool 标志, 如果有 --direct-io 参数, 则设置为 true
                .requires("mount-point") // 这个参数, 依赖于 --mount-point <MOUNT_POINT>
                .help("Mount FUSE with direct IO")
        )
        .arg(
            Arg::new("fsck")
                .long("fsck") // --fsck
                .action(ArgAction::SetTrue)
                .help("Run a filesystem check")
        )
        .arg(
            Arg::new("suid")
                .long("suid") // --suid
                .action(ArgAction::SetTrue)
                .help("Enable setuid support when run as root")
        )
        .arg(
            Arg::new("v")
                .short('v') // -v
                .action(ArgAction::Count) // -vv -vvv, 更高的日志等级
                .help("Sets the level of verbosity")
        )
        .get_matches();

    let verbosity = matches.get_count("v");
    let log_level = match verbosity {
        0 => LevelFilter::Error,
        1 => LevelFilter::Warn,
        2 => LevelFilter::Info,
        3 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    env_logger::builder().format_timestamp_nanos().filter_level(log_level).init(); // Log 只是 facade, env_logger 是实现, 里面会引用 log

    let mut options = vec![MountOption::FSName("fuser".to_string())];

    options.push(MountOption::AutoUnmount);

    if let Ok(enabled) = fuse_allow_other_enabled() {
        if enabled {
            options.push(MountOption::AllowOther);
        }
    } else {
        eprintln!("Unable to read /etc/fuse.conf");
    }

    let data_dir = matches.get_one::<String>("data-dir").unwrap().to_string();

    let mountpoint: String = matches.get_one::<String>("mount-point").unwrap().to_string();

    // 这个 fuser::mount2 好像是 阻塞的
    let result = fuser::mount2(
        SimpleFS::new(data_dir, matches.get_flag("direct-io"), matches.get_flag("suid")),
        mountpoint,
        &options
    );
    if let Err(e) = result {
        // Return a special error code for permission denied, which usually indicates that
        // "user_allow_other" is missing from /etc/fuse.conf
        if e.kind() == ErrorKind::PermissionDenied {
            error!("{}", e.to_string());
            std::process::exit(2);
        }
    }
}
