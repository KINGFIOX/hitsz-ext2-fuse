#[allow(unused_imports)]
use super::*;

use std::any::Any;

#[allow(unused)]
pub trait BlockDevice: Send + Sync + Any {
    fn read_block(&self, blockno: usize, buf: &mut [u8]);
    fn write_block(&self, blockno: usize, buf: &[u8]);
}
