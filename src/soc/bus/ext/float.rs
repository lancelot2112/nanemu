//! Floating point helpers layered on top of `DataHandle`.

use crate::soc::bus::{BusCursor, BusResult};

pub trait FloatCursorExt {
    fn read_f32(&mut self) -> BusResult<f32>;
    fn read_f64(&mut self) -> BusResult<f64>;
}

impl FloatCursorExt for BusCursor {
    fn read_f32(&mut self) -> BusResult<f32> {
        let bits = self.read_u32()?;
        Ok(f32::from_bits(bits))
    }

    fn read_f64(&mut self) -> BusResult<f64> {
        let bits = self.read_u64()?;
        Ok(f64::from_bits(bits))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::soc::bus::{BusCursor, DeviceBus};
    use crate::soc::device::{AccessContext, Device, Endianness as DeviceEndianness, RamMemory};

    fn make_cursor(bytes: &[u8]) -> BusCursor {
        let mut bus = DeviceBus::new(32);
        let memory = RamMemory::new("ram", 0x20, DeviceEndianness::Little);
        memory.write(0, bytes, AccessContext::DEBUG).unwrap();
        bus.map_device(memory, 0, 0).unwrap();
        BusCursor::attach_to_bus(Arc::new(bus), 0, AccessContext::CPU)
    }

    #[test]
    fn read_f32_round_trips() {
        let mut handle = make_cursor(&f32::to_le_bytes(3.5));
        let value = handle.read_f32().expect("f32 read");
        assert!(
            (value - 3.5).abs() < f32::EPSILON,
            "decoded value should match original literal"
        );
    }

    #[test]
    fn read_f64_round_trips() {
        let mut handle = make_cursor(&f64::to_le_bytes(-12.25));
        let value = handle.read_f64().expect("f64 read");
        assert!(
            (value + 12.25).abs() < f64::EPSILON,
            "decoded value should match original literal"
        );
    }
}
