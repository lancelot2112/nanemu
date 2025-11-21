use std::{collections::HashMap, sync::Arc};

use crate::soc::core::specification::{CoreSpec, RegisterSpec};
use crate::soc::device::BasicMemory;
use crate::soc::system::bus::{BusError, DataHandle, DeviceBus};

/// Comprehensive processor snapshot referencing a local device bus so higher
/// layers can reuse the existing data-handle abstractions for bitfield access.
pub struct CoreState {
    descriptor: Arc<CoreSpec>,
    bus: Arc<DeviceBus>,
    memory: Arc<BasicMemory>,
    registers: HashMap<String, RegisterLayout>,
    handle: DataHandle,
}

impl CoreState {
    pub fn new(descriptor: Arc<CoreSpec>) -> StateResult<Self> {
        let byte_len = descriptor.byte_len();
        let memory = Arc::new(BasicMemory::new(
            format!("{}_state", descriptor.name()),
            byte_len,
            descriptor.endianness(),
        ));
        let bus = Arc::new(DeviceBus::new(LOCAL_BUS_BUCKET_BITS));
        bus.register_device(memory.clone(), 0)?;
        let mut handle = DataHandle::new(bus.clone());
        handle.address_mut().jump(0)?;
        let registers = descriptor
            .registers()
            .iter()
            .map(|spec| (spec.name.clone(), RegisterLayout::from_spec(spec)))
            .collect();
        Ok(Self {
            descriptor,
            bus,
            memory,
            registers,
            handle,
        })
    }

    pub fn descriptor(&self) -> &CoreSpec {
        &self.descriptor
    }

    pub fn bus(&self) -> &Arc<DeviceBus> {
        &self.bus
    }

    pub fn memory(&self) -> &Arc<BasicMemory> {
        &self.memory
    }

    pub fn data_handle(&mut self) -> &mut DataHandle {
        &mut self.handle
    }

    pub fn register_layout(&self, name: &str) -> Option<RegisterLayout> {
        self.registers.get(name).copied()
    }

    pub fn read_register(&mut self, name: &str) -> StateResult<u128> {
        let layout = self
            .registers
            .get(name)
            .copied()
            .ok_or_else(|| StateError::UnknownRegister(name.to_string()))?;
        self.read_bits_at(layout.byte_offset, layout.bit_offset, layout.bit_len)
    }

    pub fn write_register(&mut self, name: &str, value: u128) -> StateResult<()> {
        let layout = self
            .registers
            .get(name)
            .copied()
            .ok_or_else(|| StateError::UnknownRegister(name.to_string()))?;
        self.write_bits_at(layout.byte_offset, layout.bit_offset, layout.bit_len, value)
    }

    pub fn read_bits_at(
        &mut self,
        byte_offset: u64,
        bit_offset: u8,
        bit_len: u16,
    ) -> StateResult<u128> {
        self.handle.address_mut().jump(byte_offset)?;
        let value = self.handle.read_bits(bit_offset, bit_len)?;
        Ok(value)
    }

    pub fn write_bits_at(
        &mut self,
        byte_offset: u64,
        bit_offset: u8,
        bit_len: u16,
        value: u128,
    ) -> StateResult<()> {
        self.handle.address_mut().jump(byte_offset)?;
        self.handle.write_bits(bit_offset, bit_len, value)?;
        Ok(())
    }

    pub fn zeroize(&mut self) -> StateResult<()> {
        self.handle.address_mut().jump(0)?;
        let buffer = vec![0u8; self.descriptor.byte_len()];
        self.handle.write(&buffer)?;
        self.handle.address_mut().jump(0)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterLayout {
    pub byte_offset: u64,
    pub bit_offset: u8,
    pub bit_len: u16,
}

impl RegisterLayout {
    fn from_spec(spec: &RegisterSpec) -> Self {
        Self {
            byte_offset: (spec.bit_offset / 8) as u64,
            bit_offset: (spec.bit_offset % 8) as u8,
            bit_len: spec.bit_len,
        }
    }
}

#[derive(Debug)]
pub enum StateError {
    Bus(BusError),
    UnknownRegister(String),
}

pub type StateResult<T> = Result<T, StateError>;

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Bus(err) => write!(f, "bus error: {err}"),
            StateError::UnknownRegister(name) => write!(f, "unknown register '{name}'"),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StateError::Bus(err) => Some(err),
            StateError::UnknownRegister(_) => None,
        }
    }
}

impl From<BusError> for StateError {
    fn from(err: BusError) -> Self {
        StateError::Bus(err)
    }
}

const LOCAL_BUS_BUCKET_BITS: u8 = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soc::device::Endianness;

    fn demo_descriptor() -> Arc<CoreSpec> {
        Arc::new(
            CoreSpec::builder("demo", Endianness::Little)
                .register("pc", 64)
                .register("sp", 64)
                .register("flags", 8)
                .build()
                .expect("descriptor"),
        )
    }

    #[test]
    fn register_round_trip() {
        let descriptor = demo_descriptor();
        let mut state = CoreState::new(descriptor).expect("core state");
        state.write_register("pc", 0xDEADBEEF).expect("write pc");
        let value = state.read_register("pc").expect("read pc");
        assert_eq!(value, 0xDEADBEEF);
    }

    #[test]
    fn register_layout_exposes_offsets() {
        let descriptor = demo_descriptor();
        let state = CoreState::new(descriptor).expect("core state");
        let pc = state.register_layout("pc").expect("pc layout");
        assert_eq!(pc.byte_offset, 0);
        assert_eq!(pc.bit_len, 64);
    }

    #[test]
    fn states_share_descriptor_without_aliasing_memory() {
        let descriptor = demo_descriptor();
        let mut first = CoreState::new(descriptor.clone()).expect("first");
        let mut second = CoreState::new(descriptor).expect("second");
        first.write_register("pc", 0x1).expect("write first");
        let second_value = second.read_register("pc").expect("read second");
        assert_eq!(second_value, 0, "independent states keep isolated memory");
    }
}
