//! Integer helpers that perform width-aware reads and sign/zero extension.

use crate::soc::device::endianness::Endianness;
use crate::soc::system::bus::{BusError, BusResult, DataHandle};

/// Trait adding width-aware integer reads directly on top of `DataHandle`.
pub trait SignedDataHandleExt {
    fn read_unsigned(&mut self, width: usize, order: Endianness) -> BusResult<u64>;
    fn read_signed(&mut self, width: usize, order: Endianness) -> BusResult<i64>;
}

impl SignedDataHandleExt for DataHandle {
    fn read_unsigned(&mut self, width: usize, order: Endianness) -> BusResult<u64> {
        ensure_width(width)?;
        let mut buf = [0u8; 8];
        if width > 0 {
            self.read_bytes(&mut buf[..width])?;
        }
        Ok(decode_unsigned(&buf[..width], order))
    }

    fn read_signed(&mut self, width: usize, order: Endianness) -> BusResult<i64> {
        let value = self.read_unsigned(width, order)?;
        Ok(sign_extend(value, (width * 8) as u32))
    }
}

pub fn decode_unsigned(bytes: &[u8], order: Endianness) -> u64 {
    let mut value = 0u64;
    match order {
        Endianness::Little => {
            for (idx, byte) in bytes.iter().enumerate() {
                value |= (*byte as u64) << (idx * 8);
            }
        }
        Endianness::Big => {
            for (idx, byte) in bytes.iter().rev().enumerate() {
                value |= (*byte as u64) << (idx * 8);
            }
        }
    }
    value
}

pub fn decode_signed(bytes: &[u8], order: Endianness) -> i64 {
    sign_extend(decode_unsigned(bytes, order), (bytes.len() * 8) as u32)
}

fn sign_extend(value: u64, bits: u32) -> i64 {
    if bits == 0 {
        return 0;
    }
    let shift = 64u32.saturating_sub(bits);
    ((value << shift) as i64) >> shift
}

fn ensure_width(width: usize) -> BusResult<()> {
    if width == 0 || width > 8 {
        return Err(BusError::DeviceFault {
            device: "bus-ext".into(),
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "integer width must be between 1 and 8 bytes",
            )),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soc::device::{BasicMemory, Device, Endianness as DeviceEndianness};
    use crate::soc::system::bus::DeviceBus;
    use std::sync::Arc;

    fn make_handle(bytes: &[u8]) -> DataHandle {
        let bus = Arc::new(DeviceBus::new(8));
        let memory = Arc::new(BasicMemory::new("ram", 0x20, DeviceEndianness::Little));
        bus.register_device(memory.clone(), 0).unwrap();
        memory.write(0, bytes).unwrap();
        let mut handle = DataHandle::new(bus);
        handle.address_mut().jump(0).unwrap();
        handle
    }

    #[test]
    fn read_unsigned_matches_expected_value() {
        let mut handle = make_handle(&[0x34, 0x12, 0, 0]);
        let value = handle
            .read_unsigned(2, Endianness::Little)
            .expect("read u16");
        assert_eq!(value, 0x1234, "little endian decode should match reference");
    }

    #[test]
    fn read_signed_sign_extends_properly() {
        let mut handle = make_handle(&[0x80]);
        let value = handle
            .read_signed(1, Endianness::Little)
            .expect("read i8");
        assert_eq!(value, -128, "sign extension should honor the MSB");
    }

    #[test]
    fn decode_helpers_work_from_slices() {
        let unsigned = decode_unsigned(&[0x01, 0x02], Endianness::Big);
        assert_eq!(unsigned, 0x0102, "decode should treat input as big endian");
        let signed = decode_signed(&[0xFF, 0xFE], Endianness::Big);
        assert_eq!(signed, -2, "signed decoding should sign-extend the buffer");
    }
}
