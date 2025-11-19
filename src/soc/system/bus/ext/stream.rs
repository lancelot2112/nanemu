//! Stream-style adapters built on top of `DataHandle` so call-sites can read contiguous regions without open-coding cursor math.

use crate::soc::system::bus::{BusResult, DataHandle};

/// Lightweight view that exposes buffered style helpers over a mutable [`DataHandle`].
pub struct DataStream<'a> {
    handle: &'a mut DataHandle,
}

impl<'a> DataStream<'a> {
    pub fn new(handle: &'a mut DataHandle) -> Self {
        Self { handle }
    }

    pub fn read_exact(&mut self, len: usize) -> BusResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        if len > 0 {
            self.handle.read_bytes(&mut buf)?;
        }
        Ok(buf)
    }

    pub fn skip(&mut self, len: u64) -> BusResult<()> {
        self.handle.address_mut().advance(len)
    }

    pub fn fill_slice(&mut self, out: &mut [u8]) -> BusResult<()> {
        if out.is_empty() {
            return Ok(());
        }
        self.handle.read_bytes(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soc::device::{BasicMemory, Device, Endianness};
    use crate::soc::system::bus::DeviceBus;
    use std::sync::Arc;

    fn make_handle() -> (DataHandle, Arc<BasicMemory>) {
        let bus = Arc::new(DeviceBus::new(8));
        let memory = Arc::new(BasicMemory::new("ram", 0x40, Endianness::Little));
        bus.register_device(memory.clone(), 0).unwrap();
        let mut handle = DataHandle::new(bus);
        handle.address_mut().jump(0).unwrap();
        (handle, memory)
    }

    #[test]
    fn read_exact_returns_requested_bytes() {
        let (mut handle, memory) = make_handle();
        memory.write(0, &[1, 2, 3, 4]).unwrap();
        let mut stream = DataStream::new(&mut handle);
        let bytes = stream.read_exact(4).expect("read snapshot");
        assert_eq!(bytes, &[1, 2, 3, 4], "stream should copy bytes verbatim");
    }

    #[test]
    fn skip_advances_underlying_cursor() {
        let (mut handle, _) = make_handle();
        let mut stream = DataStream::new(&mut handle);
        stream.skip(8).unwrap();
        assert_eq!(
            handle.address().bus_address(),
            Some(8),
            "skip should advance address cursor"
        );
    }
}
