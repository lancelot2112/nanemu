# Device Architecture

## 1. Purpose

The **Device Architecture** defines how all non-core components of the system—timers, UARTs, SPI controllers, DMA engines, interrupt controllers, memories (SRAM/Flash), MMIO blocks, and even exotic coprocessors—are represented, instantiated, wired, and executed within the emulator.

Devices:

* Participate in the scheduler like any component.
* Interact with cores via buses, interrupts, register spaces, or special interfaces.
* Expose internal registers through **MMIO** or **non-addressable regspaces** (e.g., SPR/CSR-like spaces).
* May have their own timing/clock domains.
* May be purely software-defined (behavioral models) or microcoded (via their own ISA).

The architecture aims to unify these concepts under a clear, extensible model.

---

## 2. Device Types

There are several categories of devices, all adhering to a shared abstraction:

### 2.1 Memory Devices (Specialized Devices)

These are simple devices that:

* Have linear storage (RAM, ROM, Flash).
* Respond to bus read/write requests.
* Define timing properties (read/write latencies).
* May have special behaviors (flash programming sequences, ECC).

Example characteristics:

* Backing storage buffer.
* `read_latency`, `write_latency`.
* Optional write restrictions.
* Optional page/sector behavior.

### 2.2 MMIO Devices

Devices whose internal registers are accessible through the **bus address space**.

Characteristics:

* One or more **MMIO register banks**.
* Defined by base address and size.
* Registers with bitfields and access modes.
* May raise interrupts, DMA requests, or events.

### 2.3 SPR / CSR / Non-Addressable Register Devices

These devices expose registers **not bus-addressable**, but instead accessed via instructions.

Examples:

* PowerPC SPRs
* RISC-V CSRs
* TriCore system registers
* Special coprocessor control registers

These exist in **regspaces** defined by `.isa`, and devices bind portions of those spaces.

### 2.4 Active Devices (Timers, Interrupt Controllers, DMA, State Machines)

These devices:

* Maintain internal state machines.
* Tick according to a clock domain.
* Generate interrupts.
* Push DMA/Bus requests.

### 2.5 Cores-as-Devices

Some devices are essentially cores with their own ISA:

* eTPU
* DSP blocks
* Management controllers

These use `.isa + .coredef` but register as **device instances** in `.sysdef`.

---

## 3. Device Definition Files (`.devdef`)

Devices are defined in dedicated `.devdef` files, enabling modular addition of peripherals.

### 3.1 Structure

```text
devtype timer16 {
    # Register banks (MMIO)
    registers mmio "TIMER" {
        base  0x4000_0000;
        size  0x20;

        reg CTRL  @0x00 { bits { EN:1, MODE:2, IRQ_EN:1 } }
        reg COUNT @0x04 { bits { VALUE:16 } readonly; }
        reg LOAD  @0x08 { bits { VALUE:16 } }
        reg STAT  @0x0C { bits { IRQ:1 } write1_to_clear; }
    }

    # Optional non-addressable register space
    registers spr "timer_spr" {
        reg TCFG @0 { bits { X:8 } }
    }

    # Timing / clock domain
    clock base / 4;

    # Interrupt outputs
    irq_output "timer_irq0";

    # Link to behavior implementation or DSL
    behavior "timer16_default";
}
```

A `.devdef` file defines *a reusable device type*. The `.sysdef` file instantiates it.

---

## 4. Regspaces (Register Spaces)

Devices may expose registers into **abstract regspaces**, which may be:

* MMIO-mapped
* Non-addressable (SPR/CSR-like)

Regspaces are declared in the ISA and extended in `.devdef`.

### 4.1 In `.isa`

```text
regspace SPR { count 1024; width 32; }
```

### 4.2 In `.devdef`

```text
registers spr "system_spr" {
    reg SPR_SRR0 @26 { bits { VALUE:32 } }
    reg SPR_SRR1 @27 { bits { VALUE:32 } }
}
```

### 4.3 In `.coredef`

```text
use_regspace SPR;
```

### 4.4 In `.sysdef`

```text
device sysregs0 : system_spr {
    attach regspace SPR of core0;
}
```

This cleanly separates:

* The **ISA-level specification** of register spaces.
* The **device-level population** of register indices.
* The **system-level wiring** to cores.

---

## 5. Device Behavior

Each device has a **Behavior Engine** determining how it responds when scheduled.

### 5.1 Device Trait

Every concrete device implements a narrow trait that focuses on four responsibilities:

1. Reporting metadata (`name`, `span`, `endianness`).
2. Handling *bit-slice* reads/writes.
3. Ticking optional internal state machines.
4. Exposing side channels (IRQs, DMA, etc.).

All bus-visible data transfers, from single-bit flags to wide cache lines, go through a single pair of functions:

```rust
trait Device: Component {
    fn read_bits(
        &self,
        byte_offset: u64,
        burst_bytes: usize,
        bit_offset: Option<u8>,
        bit_len: Option<u16>,
        out: &mut [u64],
    ) -> DeviceResult<()>;

    fn write_bits(
        &self,
        byte_offset: u64,
        burst_bytes: usize,
        bit_offset: Option<u8>,
        bit_len: Option<u16>,
        data: &[u8],
    ) -> DeviceResult<()>;

    fn on_tick(&mut self, now: u64, sys: &mut System);
    fn irq_lines(&self) -> &[IrqLine];
}
```

* **Single surface area.** All higher-level helpers (`read_u32`, string dumps, MMIO field access, etc.) are implemented by the bus translation layer. No more proliferation of `read_u8`, `read_u16`, `read_bytes`, etc. inside devices.
* **Bit precision.** `bit_len` may span any number of bits (up to 128 today, expandable later). Devices return/accept *unmasked* aligned bursts in their native byte order. Optional `bit_offset`/`bit_len` parameters tell the bus cache which subset of the burst the caller cares about; the cache handles all masking, merging, and byte swapping.
* **Native transport.** Devices store their state in whatever byte order is natural for the emulated target. The `BitSliceCache` keeps both the device-native view and a lazily converted target view, doing the endian shuffle exactly once per burst. Writes follow the inverse path: the cache expands the caller’s byte slice, applies the computed bit mask, and passes aligned bytes plus an optional mask down to the device.

### 5.2 Bit-Slice Translation Layer

Between the device trait and the rest of the system lives a small *translation/cache layer* owned by the bus:

* It converts `(byte_offset, burst_bytes, bit_offset, bit_len)` into one or two aligned reads.
* It keeps a **BitSliceCache** that stores both device-order and target-order representations of the fetched words. The cache only flips bytes when the caller asks for target order, avoiding repeated conversions for string dumps or raw byte views.
* Scalar helpers (e.g., `read_u32`) simply mask/shift within the cached words. If consecutive accesses hit the same slice, no additional device call or reordering is needed.
* Writes follow the same pattern: the cache accepts an arbitrary byte slice, optionally scoped by `(bit_offset, bit_len)`, expands it to the burst size, flips bytes if needed, and forwards the final bytes (and optional mask) to the device.

This arrangement gives us a single, well-documented API while still enabling high-performance bitfield operations and zero-copy raw reads.

### 5.2 Behavior Sources

Behavior can come from:

* Built-in Rust implementations (fastest, safest)
* Plugin modules
* A small device behavior DSL (future)
* Microcoded device cores (via their own ISA)

---

## 6. Device Integration in `.sysdef`

Devices are instantiated and connected using system definitions.

```text
system mpc5777c {
    device timer0 : timer16 {
        attach bus "ahb0";
        irq -> intc.line(5);
    }

    device flash0 : flash_mem {
        attach bus "ahb0";
    }

    device sysregs0 : system_spr {
        attach regspace SPR of core0;
    }
}
```

### 6.1 Bus Attachment

* Determines which requests the device will respond to.
* Configures arbitration and timing behavior.

### 6.2 Register-Space Attachment

* Binds SPR/CSR regspaces to one or more cores.
* Allows instructions like `mtspr` or `mfcr` to hit the correct device.

### 6.3 Interrupt Wiring

* Device outputs map to interrupt controller inputs.
* System builder validates mappings.

---

## 7. Timing & Clock Domains

Devices may have:

* Their own clock rate (`clock base / N`).
* Internal timers that fire at predictable cycles.
* Latencies for reads/writes.
* State machine transitions.

The scheduler invokes each device according to its clock divisor.

---

## 8. Device as Unified Concept

This architecture unifies memory, MMIO, SPRs, coprocessors, and active devices under a consistent framework:

* **Everything is a Device** with optional:

  * Address-mapped registers (MMIO)
  * Non-addressable regspaces (SPRs)
  * Behavior/state-machine logic
  * Timing/clock domain
  * Interrupt/DMA outputs

* The Bus only routes requests.

* The Executor only applies device responses.

* The Scheduler advances device state.

* The System Builder wires devices to cores and buses.

---

## 9. Summary

The Device Architecture:

* Defines a unified, extensible way to add peripherals and memory.
* Decouples ISA, core definition, and device behavior.
* Supports MMIO, SPR/CSR spaces, and coprocessor-style devices.
* Integrates naturally with buses, scheduler, and execution engine.
* Provides a clean long-term path for modeling real SoCs.

This completes the fourth major pillar alongside the **Decoder**, **Execution Engine**, and **Scheduler** architectures.
