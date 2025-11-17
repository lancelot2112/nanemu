use std::{ops::Range, sync::RwLock};

use crate::soc::device::{Device, DeviceError, DeviceResult, Endianness};

pub struct BasicMemory {
    name: String,
    bytes: RwLock<Vec<u8>>,
    endian: Endianness,
}

impl BasicMemory {
    pub fn new(name: impl Into<String>, size: usize, endian: Endianness) -> Self {
        Self {
            name: name.into(),
            bytes: RwLock::new(vec![0_u8; size]),
            endian,
        }
    }

    pub fn size(&self) -> u64 {
        self.bytes.read().unwrap().len() as u64
    }
}

impl Device for BasicMemory {
    fn name(&self) -> &str {
        &self.name
    }

    fn span(&self) -> Range<u64> {
        0..self.size()
    }

    fn endianness(&self) -> Endianness {
        self.endian
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> DeviceResult<()> {
        let len = buf.len() as u64;
        let data = self.bytes.read().unwrap();
        if offset + len > data.len() as u64 {
            return Err(DeviceError::OutOfRange {
                offset,
                len,
                capacity: data.len() as u64,
            });
        }
        let start = offset as usize;
        let end = start + buf.len();
        buf.copy_from_slice(&data[start..end]);
        Ok(())
    }

    fn write(&self, offset: u64, data_in: &[u8]) -> DeviceResult<()> {
        let len = data_in.len() as u64;
        let mut data = self.bytes.write().unwrap();
        if offset + len > data.len() as u64 {
            return Err(DeviceError::OutOfRange {
                offset,
                len,
                capacity: data.len() as u64,
            });
        }
        let start = offset as usize;
        let end = start + data_in.len();
        data[start..end].copy_from_slice(data_in);
        Ok(())
    }
}