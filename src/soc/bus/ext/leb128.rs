//! LEB128 read/write helpers reused by symbol and loader tooling.
use crate::soc::bus::{BusCursor, BusResult};

pub trait Leb128CursorExt {
    fn read_uleb128(&mut self) -> BusResult<(u64, usize)>;
    fn read_sleb128(&mut self) -> BusResult<(i64, usize)>;
}

impl Leb128CursorExt for BusCursor {
    fn read_uleb128(&mut self) -> BusResult<(u64, usize)> {
        let mut result = 0u64;
        let mut shift = 0;
        let cursor = self.get_position();
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u64) << shift;
            if (byte & 0x80) == 0 {
                break;
            }
            shift += 7;
        }
        Ok((result, self.get_position() - cursor))
    }

    fn read_sleb128(&mut self) -> BusResult<(i64, usize)> {
        let mut result = 0i64;
        let mut shift = 0;
        let mut byte;
        let cursor = self.get_position();
        loop {
            byte = self.read_u8()? as i64;
            result |= (byte & 0x7F) << shift;
            shift += 7;
            if (byte & 0x80) == 0 {
                break;
            }
        }
        if (shift < 64) && ((byte & 0x40) != 0) {
            result |= !0 << shift;
        }
        Ok((result, self.get_position() - cursor))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::soc::bus::DeviceBus;
    use crate::soc::device::{AccessContext, Device, Endianness, RamMemory};

    fn make_cursor(bytes: &[u8]) -> BusCursor {
        let mut bus = DeviceBus::new(32);
        let memory = RamMemory::new("rom", 0x20, Endianness::Little);
        memory.write(0, bytes, AccessContext::DEBUG).unwrap();
        bus.map_device(memory, 0, 0).unwrap();
        BusCursor::attach_to_bus(Arc::new(bus), 0, AccessContext::CPU)
    }

    #[test]
    fn read_uleb128_decodes_example() {
        let mut cursor = make_cursor(&[0xE5, 0x8E, 0x26]);
        let (value, size) = cursor.read_uleb128().expect("uleb");
        assert_eq!(
            value, 624485,
            "ULEB128 example from DWARF spec should parse"
        );
        assert_eq!(size, 3, "should consume three bytes");
    }

    #[test]
    fn read_sleb128_decodes_negative_example() {
        let mut cursor = make_cursor(&[0x9B, 0xF1, 0x59]);
        let (value, size) = cursor.read_sleb128().expect("sleb");
        assert_eq!(
            value, -624485,
            "SLEB128 example from DWARF spec should parse"
        );
        assert_eq!(size, 3, "should consume three bytes");
    }
}
