//! Integer helpers that perform width-aware reads and sign/zero extension on top of `DataHandle`.

use crate::soc::bus::{BusCursor, BusResult};

/// Trait adding width-aware integer reads directly on top of `DataHandle`.
pub trait SignedCursorExt {
    fn read_i8(&mut self) -> BusResult<i8>;
    fn read_i16(&mut self) -> BusResult<i16>;
    fn read_i32(&mut self) -> BusResult<i32>;
    fn read_i64(&mut self) -> BusResult<i64>;
}

impl SignedCursorExt for BusCursor {
    fn read_i8(&mut self) -> BusResult<i8> {
        let raw = self.read_u8()?;
        Ok(raw as i8)
    }
    fn read_i16(&mut self) -> BusResult<i16> {
        let raw = self.read_u16()?;
        Ok(raw as i16)
    }
    fn read_i32(&mut self) -> BusResult<i32> {
        let raw = self.read_u32()?;
        Ok(raw as i32)
    }
    fn read_i64(&mut self) -> BusResult<i64> {
        let raw = self.read_u64()?;
        Ok(raw as i64)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::soc::bus::{BusCursor, DeviceBus};
    use crate::soc::device::{AccessContext, Device, Endianness as DeviceEndianness, RamMemory};

    fn make_cursor(bytes: &[u8], endian: DeviceEndianness) -> BusCursor {
        let mut bus = DeviceBus::new(32);
        let memory = RamMemory::new("ram", 0x20, endian);
        memory
            .write(0, bytes, AccessContext::DEBUG)
            .expect("write preload");
        bus.map_device(memory, 0, 0).unwrap();
        BusCursor::attach_to_bus(Arc::new(bus), 0, AccessContext::CPU)
    }

    #[test]
    fn read_i8_sign_extends_negative_values() {
        let mut cursor = make_cursor(&[0xFE], DeviceEndianness::Little);
        let value = cursor.read_i8().expect("read i8");
        assert_eq!(value, -2, "0xFE should sign extend to -2");
    }

    #[test]
    fn read_i16_sign_extends_on_little_endian() {
        let source = [0x34, 0xFF];
        let mut cursor = make_cursor(&source, DeviceEndianness::Little);
        let value = cursor.read_i16().expect("read i16");
        assert_eq!(value, i16::from_le_bytes(source), "bytes should round-trip");
    }

    #[test]
    fn read_i32_respects_big_endian_layout() {
        let source = [0xFE, 0xDC, 0xBA, 0x98];
        let mut cursor = make_cursor(&source, DeviceEndianness::Big);
        let value = cursor.read_i32().expect("read i32");
        assert_eq!(
            value,
            i32::from_be_bytes(source),
            "big-endian bytes should sign extend correctly"
        );
    }

    #[test]
    fn read_i64_consumes_full_width_and_sign_extends() {
        let source = [0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0xF1];
        let mut cursor = make_cursor(&source, DeviceEndianness::Little);
        let value = cursor.read_i64().expect("read i64");
        assert_eq!(
            value,
            i64::from_le_bytes(source),
            "little-endian bytes should sign extend correctly"
        );
        assert_eq!(
            cursor.get_position(),
            8,
            "reading i64 should advance the cursor by 8 bytes",
        );
    }
}
