#[allow(unused_imports)]
use super::*;

use std::any::Any;

#[allow(unused)]
pub trait BlockDevice: Send + Sync + Any {
    fn read_block(&self, block_id: usize, buf: &mut [u8]);
    fn write_block(&self, block_id: usize, buf: &[u8]);
}
