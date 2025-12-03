//! BusCursor wraps a resolved bus range and provides cursor-based navigation
//! so callers can keep a stable cursor across jumps, reads, and writes without
//! mutating the underlying `DeviceBus` mapping.
//!
//! The handle owns an `Arc<DeviceBus>` and validates bounds for every cursor
//! movement, mirroring the responsibilities of `BasicBusAccess` in the .NET
//! reference implementation while remaining borrowing-friendly for Rust.
//! It also provides a `transact` method that simplifies performing
//! read/write operations against the currently mapped device at the current cursor
//! position simulating atomicity.
use std::sync::Arc;

use crate::soc::{bus::{DeviceBus, softmmu::SoftMMU, softtlb::SoftTLB}, device::AccessContext};

use super::error::BusResult;

pub struct BusCursor{
    tlb: SoftTLB,
    ref_zero: usize,
    address: usize,
}

impl BusCursor {
    pub fn new(tlb: SoftTLB, start: usize) -> Self {
        Self {
            tlb,
            ref_zero: start,
            address: start,
        }
    }

    pub fn attach_to_bus(bus: Arc<DeviceBus>, start: usize, context: AccessContext) -> Self {
        let mmu = SoftMMU::new(bus);
        let tlb = SoftTLB::new(Arc::new(mmu), context);
        Self::new(tlb, start)
    }

    #[inline(always)]
    fn validate_request(&mut self, requested: usize) -> BusResult<()> {
        let _entry = self.tlb.lookup(requested)?;
        Ok(())
    }

    // General purpose jump to an absolute offset within the mapped range.
    #[inline(always)]
    pub fn goto(&mut self, new_offset: usize) -> BusResult<usize> {
        if new_offset == self.address {
            return Ok(self.address);
        }

        match self.validate_request(new_offset) {
            Ok(_) => {
                self.address = new_offset;
                Ok(self.address)
            }
            Err(e) => {
                Err(e)
            }
        }
    }

    // Pin a cursor position within the mapped range.  Pin is initially set to the offset at new.
    #[inline(always)]
    pub fn set_ref(&mut self, new_offset: usize) -> BusResult<usize> {
        self.goto(new_offset)?;
        self.ref_zero = self.address;
        Ok(self.ref_zero)
    }

    // forward or backwarde cursor relative to the pinned position.
    #[inline(always)]
    pub fn forward_from_ref(&mut self, delta: usize) -> BusResult<usize> {
        self.goto(self.ref_zero.saturating_add(delta))
    }

    #[inline(always)]
    pub fn backward_from_ref(&mut self, delta: usize) -> BusResult<usize> {
        self.goto(self.ref_zero.saturating_sub(delta))
    }

    #[inline(always)]
    pub fn goto_ref(&mut self) {
        // Reset cursor to pinned position... it's already been validated 
        // so no need to repeat work
        self.address = self.ref_zero;
    }

    // forward or backwarde cursor relative to the current cursor.
    #[inline(always)]
    pub fn forward(&mut self, delta: usize) -> BusResult<usize> {
        self.goto(self.address.saturating_add(delta))
    }

    #[inline(always)]
    pub fn backward(&mut self, delta: usize) -> BusResult<usize> {
        self.goto(self.address.saturating_sub(delta))
    }

    #[inline(always)]
    pub fn dist_from_ref(&self) -> isize {
        self.address as isize - self.ref_zero as isize
    }

    #[inline(always)]
    pub fn get_ref(&self) -> usize {
        self.ref_zero
    }

    #[inline(always)]
    pub fn get_position(&self) -> usize {
        self.address
    }

    #[inline(always)]
    pub fn peek_u8(&mut self) -> BusResult<u8> {
        self.tlb.peek::<u8>(self.address)
    }

    #[inline(always)]
    pub fn peek_u16(&mut self) -> BusResult<u16> {
        self.tlb.peek::<u16>(self.address)
    }

    #[inline(always)]
    pub fn peek_u32(&mut self) -> BusResult<u32> {
        self.tlb.peek::<u32>(self.address)
    }

    #[inline(always)]
    pub fn peek_u64(&mut self) -> BusResult<u64> {
        self.tlb.peek::<u64>(self.address)
    }

    #[inline(always)]
    pub fn peek_ram(&mut self, size: usize) -> BusResult<&[u8]> {
        self.tlb.read_ram(self.address, size)
    }

    #[inline(always)]
    pub fn read_ram(&mut self, size: usize) -> BusResult<&[u8]> {
        let out = self.tlb.read_ram(self.address, size)?;
        self.address += size;
        Ok(out)
    }

    #[inline(always)]
    pub fn write_ram(&mut self, data: &[u8]) -> BusResult<()> {
        self.tlb.write_ram(self.address, data)?;
        self.address += data.len();
        Ok(())
    }

    #[inline(always)]
    pub fn read_u8(&mut self) -> BusResult<u8> {
        let value = self.tlb.read::<u8>(self.address)?;
        self.address += 1;
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u16(&mut self) -> BusResult<u16> {
        let value = self.tlb.read::<u16>(self.address)?;
        self.address += 2;
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u32(&mut self) -> BusResult<u32> {
        let value = self.tlb.read::<u32>(self.address)?;
        self.address += 4;
        Ok(value)
    }

    #[inline(always)]
    pub fn read_u64(&mut self) -> BusResult<u64> {
        let value = self.tlb.read::<u64>(self.address)?;
        self.address += 8;
        Ok(value)
    }

    #[inline(always)]
    pub fn write_u8(&mut self, value: u8) -> BusResult<()> {
        self.tlb.write::<u8>(self.address, value)?;
        self.address += 1;
        Ok(())
    }

    #[inline(always)]
    pub fn write_u16(&mut self, value: u16) -> BusResult<()> {
        self.tlb.write::<u16>(self.address, value)?;
        self.address += 2;
        Ok(())
    }

    #[inline(always)]
    pub fn write_u32(&mut self, value: u32) -> BusResult<()> {
        self.tlb.write::<u32>(self.address, value)?;
        self.address += 4;
        Ok(())
    }

    #[inline(always)]
    pub fn write_u64(&mut self, value: u64) -> BusResult<()> {
        self.tlb.write::<u64>(self.address, value)?;
        self.address += 8;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::soc::bus::{BusCursor, DeviceBus, SoftMMU, SoftTLB};
    use crate::soc::device::{AccessContext, Endianness, RamMemory};

    fn make_bus() -> DeviceBus {
        let mut bus = DeviceBus::new(32);
        let memory = RamMemory::new("ram", 0x2000, Endianness::Little);
        bus.map_device(memory, 0x1000, 0).expect("map device");
        bus
    }

    fn make_cursor() -> BusCursor {
        let bus = make_bus();
        let mmu = SoftMMU::new(Arc::new(bus));
        let tlb = SoftTLB::new(Arc::new(mmu), AccessContext::CPU);
        BusCursor::new(tlb, 0x1000)
    }

    #[test]
    fn move_relative_cursor() {
        let mut cursor = make_cursor();
        assert_eq!(
            cursor.get_position(),
            0x1000,
            "cursor should align with the jump address"
        );
        assert_eq!(
            cursor.forward(0x10).unwrap(),
            0x1010,
            "forward should move cursor forward by requested bytes"
        );
        
        assert_eq!(
            cursor.backward(0x8).unwrap(),
            0x1008,
            "backward pulls cursor back within the range"
        );
        assert!(
            cursor.backward(0x9).is_err(),
            "backward past mapping start should error"
        );

        assert_eq!(
            cursor.get_position(),
            0x1008,
            "error'd cursor was not moved"
        );

        assert!(
            cursor.forward(0x1FF8).is_err(),
            "forward past mapping end should error"
        );

        assert_eq!(
            cursor.get_position(),
            0x1008,
            "error'd cursor was not moved"
        );

        assert!(
            cursor.goto(0x3000).is_err(),
            "absolute jump past mapping end should error"
        );

        assert_eq!(
            cursor.get_position(),
            0x1008,
            "error'd cursor was not moved"
        );

        assert_eq!(
            cursor.goto(0x2000).unwrap(),
            0x2000,
            "absolute jump to mapping end should succeed"
        );

        assert_eq!(
            cursor.goto(0x1000).unwrap(),
            0x1000,
            "absolute jump to mapping start should succeed"
        );

    }

    #[test]
    fn move_relative_ref() {
        let mut cursor = make_cursor();
        cursor.set_ref(0x20).unwrap();
        // Positive deltas move the cursor forward, but enormous negatives are rejected.
        assert!(
            cursor.forward(0x10).is_ok(),
            "relative forward jump within mapping should succeed"
        );
        assert_eq!(
            cursor.get_position(),
            0x30,
            "cursor reflects the new relative address"
        );
        assert!(
            cursor.forward_from_ref(0x5).is_ok(),
            "relative forward from pin should succeed"
        );
        assert_eq!(
            cursor.get_position(),
            0x25,
            "cursor reflects the new relative address"
        );
        assert!(
            cursor.backward_from_ref(0x100).is_err(),
            "large negative jump should exceed bounds"
        );
        assert!(
            cursor.backward_from_ref(0x10).is_ok(),
            "relative backward from pin within bounds should succeed"
        );
        assert!(
            cursor.get_position() == 0x10,
            "cursor reflects the new relative address"
        );
        assert!(
            cursor.backward_from_ref(0x20).is_ok(),
            "relative backward to zero should succeed"
        );
        assert!(
            cursor.get_position() == 0x0,
            "cursor reflects the new relative address"
        );
    }
}
