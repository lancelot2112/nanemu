# Decoder Architecture Design

## 1. Purpose

The decoder is one of the fundamental blocks in the emulator. Its responsibilities are to:

* Determine **which machine instruction** is present at a given PC.
* Decode and bind operands into a normalized representation.
* **Lift** the instruction into the micro-IR ("risc-ish" internal representation).
* Attach an abstract **timing class** for later resolution by core and system timing models.

The decoder does **not**:

* Decide the final number of cycles an instruction takes.
* Model pipeline hazards or memory/bus latencies directly.

Those concerns belong to the core (`.coredef`) and system (`.sysdef`) configuration and the execution engine.

---

## 2. Inputs & Outputs

### 2.1 Inputs

1. **ISA Specification** (`.isa`, `.isaext`)

   * Pattern entries (mask/value) per width and group.
   * Operand extraction rules (bit fields → operand types: registers, immediates, offsets, etc.).
   * Semantic templates expressed in the semantic DSL.
   * Instruction-level **timing classes** (e.g. `alu_1c`, `branch`, `load`, `store`, `mul`, `div`, `fp_add`).

2. **Decoder Configuration (Generated)**

   * For each `(SizeClass, GroupId)` pair, a prebuilt **decode table** or decision tree.
   * Mapping from instruction IDs →

     * Semantic templates
     * Operand schemas
     * Timing class IDs

3. **Runtime Context**

   * Program counter (`pc: u64`).
   * Memory access helper(s) to fetch instruction bytes.

### 2.2 Outputs

The decoder returns a `DecodedInstr` structure (conceptual API):

* `pc: u64` — address of the instruction.
* `size: u8` — size in bytes (e.g. 2, 4, ...).
* `instr_id: InstrId` — stable identifier into ISA tables.
* `operands: SmallVec<Operand>` — normalized operands.
* `ir: SmallVec<MicroOp>` — instantiated micro-IR program for this instruction.
* `timing_class: TimingClassId` — abstract timing class (no concrete cycles yet).

This is the unit the execution engine interprets or JITs.

---

## 3. Pattern Representation & Decode Tables

### 3.1 Size Classes & Groups

We allow variable-width instructions (e.g. VLE 16/32-bit, TriCore 32-bit). Internally the decoder organizes patterns by:

* `SizeClass`: `Bits16`, `Bits32`, `Bits48`, ...
* `GroupId`: logical grouping (e.g. `root`, `vle32_group1`).

Patterns are compiled from `.isa` / `.isaext` into `PatternEntry` objects:

```text
SizeClass  — e.g. Bits16, Bits32
GroupId    — logical group (root, vle32_group1, ...)
mask: u64
value: u64
kind:
  - LeafInstr(InstructionId)
  - ExtendTo { next_size: SizeClass, group_id: GroupId }
```

* `LeafInstr` indicates a fully-decoded instruction at this width.
* `ExtendTo` marks a prefix / mini state-machine transition that widens the instruction (e.g. 16 → 32 bits) and defers final decode to another table.

### 3.2 Sorting & Specificity

For each `(SizeClass, GroupId)` pair we produce a `DecodeTable`:

* Contains sorted `PatternEntry`s.
* Sorted primarily by **mask specificity** (popcount of mask bits set):

  * More specific patterns (more fixed bits) match first.
* Optional secondary priority (explicit "priority" field) for tie-breaking.

From this we can either:

* Generate straight-line matching code over the sorted patterns, or
* Build a compressed decision tree / trie for performance.

---

## 4. Decode Algorithm (Multi-Width Support)

The decoder follows a three-phase pipeline per instruction: **fetch & classify**, **operand binding**, and **semantic instantiation**.

### 4.1 Fetch & Classify

1. **Initialize**

   * `size = initial_size_class` (e.g. `Bits16` for VLE, `Bits32` for TriCore).
   * `group = ROOT_GROUP`.

2. **Fetch minimal width**

   * Read the first chunk from memory:

     ```text
     w0 = mem.read_u16(pc)      // or read_u32 for fixed-width ISAs
     ```

3. **Lookup in decode table**

   * Use `(size, group)` to select the `DecodeTable`.
   * Match `w0` (or a transformed value) against the table.

4. **Handle match result**

   ```text
   match entry.kind:
     LeafInstr(instr_id):
         size_bytes = width_in_bytes(size)
         return decoded(instruction_id, pc, size_bytes, w0, ...)

     ExtendTo { next_size, group_id }:
         size = next_size
         group = group_id
         // fetch additional chunks and try again
   ```

5. **Extended width**

   * For 16 → 32-bit:

     ```text
     w1 = mem.read_u16(pc + 2)
     combined = (w0 << 16) | w1
     entry2 = match_table(Bits32, group, combined)
     ```

   * `entry2` must be a `LeafInstr`; nested `ExtendTo` can be supported in principle but is optional.

This process is the general-purpose variable-width decode logic that handles VLE-style mini state machines cleanly.

### 4.2 Operand Binding

Once we have `instr_id`, we:

1. Look up the **operand schema** for this instruction in the ISA metadata.
2. Extract operands from the raw bits (e.g. `w0`, `combined`):

   * Register indices
   * Immediates (sign-extended or zero-extended)
   * Displacements and offsets
   * Condition codes, flags, etc.
3. Normalize them into a `SmallVec<Operand>`:

   ```text
   Operand::Reg(RegClass, index)
   Operand::ImmSigned(width, value)
   Operand::ImmUnsigned(width, value)
   Operand::Mem { base_reg, offset, scale, ... }
   ```

### 4.3 Semantic Instantiation (Lift to Micro-IR)

Each instruction in `.isa` has a semantic template, expressed in a DSL, which is compiled ahead-of-time into a parameterized micro-IR template.

At decode time, the decoder:

1. Fetches the micro-IR template for `instr_id`.
2. Substitutes concrete operand indices and immediates.
3. Produces a `SmallVec<MicroOp>` IR program for this specific instruction instance.
4. Attaches the instruction's **timing class** (from ISA metadata).

Example micro-IR sequence for a simple register add:

```text
Op::Add32 { dst: rD, a: rA, b: rB }
Op::SetFlagEqZero { src: rD, flag: Z }
Op::SetFlagSignBit { src: rD, flag: N }
```

---

## 5. Timing Classes and Ownership

### 5.1 Where Timing Lives

Timing is split into three layers:

1. **ISA level** (`.isa`, `.isaext`)

   * Assigns each instruction a **timing class**, not concrete cycle counts.
   * Examples:

     * `alu_1c` — single-cycle ALU-style operation.
     * `alu_slow` — multi-cycle ALU.
     * `branch` — branch/PC-modifying instructions.
     * `load` / `store` — memory operations.
     * `mul`, `div`, `fp_add`, `fp_mul` — specialized units.
   * Expresses semantic complexity and functional-unit grouping.

2. **Core definition** (`.coredef`)

   * Maps **timing classes** to core-specific cycle counts and pipeline behavior.

   * Example syntax:

     ```text
     timing_class alu_1c  { latency 1; pipe "alu"; }
     timing_class mul_3c  { latency 3; pipe "mul"; }
     timing_class branch  { latency 1; may_flush true; }
     timing_class load    { latency mem; issue_stage "EX"; }
     ```

   * `latency mem` means final latency depends on memory/bus timing.

3. **System definition** (`.sysdef`)

   * Specifies memory and bus timings:

     * Memory regions with `read_latency` and `write_latency`.
     * Bus arbitration and number of ports.
     * Core and device clock frequencies/dividers.
   * Determines the **effective latency** for memory-related timing classes.

### 5.2 Decoder’s Role in Timing

The decoder’s timing responsibilities are intentionally minimal:

* It **assigns the timing class** to each decoded instruction.
* It ensures memory-related micro-IR ops are clearly marked (e.g. `IssueMemLoad`, `WaitMem`).

The decoder does **not**:

* Translate timing classes into cycle counts.
* Apply memory or bus latency rules.

Those tasks are handled by the execution engine using data from `.coredef` and `.sysdef`.

This separation allows the same ISA definitions to be reused across different cores and systems while changing timing behaviors.

---

## 6. Error Handling & Debugging

The decoder should provide rich diagnostic information for:

* **No match** in any decode table for a given bit pattern.
* **Ambiguous matches** if pattern tables are incorrectly specified.
* Invalid or reserved encodings.

Diagnostic outputs should include:

* PC, raw instruction bits.
* Size class / group used.
* Nearest matching patterns (if useful).
* Instruction ID or group context.

This is essential both for ISA bring-up and for verifying that encoder/decoder pairs are consistent.

---

## 7. Integration with the Execution Engine

* The execution engine uses `DecodedInstr` to:

  * Advance PC by `size` bytes (unless overridden by branch/jump micro-ops).
  * Execute the micro-IR in either an interpreter or JIT.
  * Use `timing_class` to determine how many cycles to charge.
* Hooks can be installed at the decoder level for:

  * Instruction tracing (PC, raw bits, instruction name).
  * Pre-instruction callbacks for fuzzing and preemption injection.

The decoder is designed to be **pure and deterministic** given the backing memory and ISA definitions, providing a stable foundation for testing and verification.
