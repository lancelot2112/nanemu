# `soc::prog::symbols` Architecture

## intent

Translate the .NET `Symbol`/`SymbolTable` system into Rust primitives that pair tightly with the new `soc::prog::types` module. The symbol layer must represent every addressable/program object discovered in ELF, DWARF, A2L, tooling metadata, or future sources while staying cache friendly, queryable, and flexible enough to support metadata-only entries.

Guiding goals:

- **Performance first** – hot-path queries (by label, id, address) stay O(1) through compact indices and cache-aware storage.
- **Expressive metadata** – symbols capture provenance (source masks, trust), visibility/binding info, storage class, and references into the shared type arena.
- **Multi-source fusion** – ingestion logic merges ELF + calibration/A2L + tool-sourced data the same way the C# table does, but with deterministic precedence and conflict resolution hooks.
- **Builder ergonomics** – tests and loaders can construct symbol trees through a fluent builder API mirroring the new type builder style.

## module layout (proposed)

```
src/soc/prog/symbols/
├── mod.rs              # re-exports + module glue
├── id.rs               # SymbolId, SymbolHandle, Strong/Weak ids
├── symbol.rs           # Symbol struct, enums (state, binding, visibility, storage)
├── source.rs           # Source flags, trust levels, provenance structs
├── table.rs            # SymbolTable storage, indices, query API
├── builder.rs          # Fluent builder for symbol creation + merge helpers
├── loader/
│   ├── elf.rs          # ELF/DWARF ingestion + linking helpers
│   ├── a2l.rs          # ASAM/A2L + tool metadata integration
│   ├── index.rs        # Tool-provided index-table linking logic
│   └── merge.rs        # Conflict-resolution strategies
├── query.rs            # Streaming iterators, filters (runtime, file, metadata, calibratable)
├── fmt.rs              # Debug output + pretty printers
└── storage.rs          # Memory managers for large symbol arenas (optional, e.g., byte-order metadata)
```

Breaking functionality along these seams keeps `table.rs` lean (just storage + indexes) while each loader handles its own parsing concerns.

## core data model

### identifiers & handles

```rust
#[repr(transparent)]
pub struct SymbolId(NonZeroU64);

pub struct SymbolHandle(u32);
```

- `SymbolId` reflects stable ids from external sources (DWARF DIE offsets, A2L parameter ids, etc.). `SymbolHandle` indexes into the table’s dense `Vec<SymbolRecord>` for fast lookups.
- A `SymbolRecord` stores `LabelId`, `TypeId`, addresses, sizes, and metadata flags. Type relationships rely on the shared `TypeArena` to avoid duplication.

### symbol structure

```rust
pub struct SymbolRecord {
    pub label: LabelId,
    pub type_id: Option<TypeId>,
    pub state: SymbolState,
    pub source: SymbolSource,
    pub binding: SymBinding,
    pub kind: SymKind,
    pub storage: StorageClass,
    pub visibility: SymVisibility,
    pub runtime_addr: u64,
    pub file_addr: u64,
    pub size: u32,
    pub section: Option<SectionHandle>,
    pub compilation_unit: Option<CuHandle>,
    pub info: Option<SymbolInfo>,
    pub byte_order: ByteOrder,
}
```
Key traits:
Key traits:

- `SymbolSource` is a bitset (ELF, A2L, Tool, Manual, etc.) used for trust decisions and auditing.
- `SymbolInfo` (optional) captures tool/A2L-specific descriptors (calibration flags, descriptions, engineering units, index-table ids).
- `StorageClass` replicates ROM/RAM/META semantics while adding `RuntimeOnly` and `OfflineAccessible` flags derived from loader metadata.

### indices & caches

`table.rs` maintains:

- `FastHashMap<LabelId, SymbolHandle>` for global labels.
- `FastHashMap<SymbolId, SymbolHandle>` for parameter IDs.
- `FastHashMap<LabelId, SmallVec<[SymbolHandle; 2]>>` for static/overloaded labels.
- `FastHashMap<LabelId, TypeId>` to expose type lookups by label (used by UI search experiences).
- Range index for address lookups: `IntervalTree<SymbolHandle>` or a `Vec` sorted by runtime/file address plus binary search helpers.

Indices update atomically when new symbols are linked or merged; we avoid per-query allocation by using `SmallVec` and reusing iterators.

## ingestion & merging

Loader flow mirrors the .NET approach but is explicit about ordering and trust:

1. **ELF/DWARF pass** – parse symbol table + CU info, allocate `SymbolRecord`s with binding/visibility/section metadata, but leave tool/A2L fields empty.
2. **A2L/tool pass** – build `SymbolRecord`s for calibration parameters, run the merge strategy to combine with existing ELF entries (preferring ELF addresses/sizes, A2L metadata/ids). Merge hooks live in `loader/merge.rs` so decisions are centralized.
3. **Index table linking** – for tool-supplied index tables (e.g., calibration maps), update runtime/file addresses and sizes, marking the table as validated just like `SymbolTable.LinkIndexTable` did.
4. **Final link** – perform post-processing (set `TypeId` for metadata-only symbols, resolve `_init` sections, convert NoType -> Function/Object when DWARF says so).

All loader stages operate on `Builder` handles, so partially built symbols never leak until `finish()` commits them into the table.

## builder ergonomics

`builder.rs` exposes a fluent API similar to the type builder:

```rust
let symbol = table.builder()
    .label("ENGINE_SPEED")
    .source(SymbolSource::Elf | SymbolSource::Ecfg)
    .binding(SymBinding::Global)
    .kind(SymKind::Object)
    .storage(StorageClass::ROM | StorageClass::OfflineAccessible)
    .runtime_addr(0x4000_0000)
    .file_addr(0x0010_0000)
    .size(4)
    .type_id(Some(u32_type))
    .info(SymbolInfo::calibratable(index_table_id))
    .finish();
```

The builder updates indices when committed and returns a `SymbolHandle`. Additional helpers: `with_section`, `with_compilation_unit`, `mark_metadata`, `attach_description`, `attach_tool_flags` (units, scaling, or other tool-only hints).

## query API

`query.rs` offers composable iterators + filters:

- `table.globals()` yields global symbols.
- `table.statics(label)` returns `impl Iterator<Item = SymbolHandle>`.
- `table.file_symbols()` vs. `table.runtime_symbols()` replicates the .NET classification.
- Address lookups use `table.at_runtime(addr)` or `table.at_file(addr)` and return either a single handle or an iterator when overlapping results exist.

To minimize allocations, iterators borrow from the table and use `SmallVec` for intermediate buffers only when ambiguous ranges occur.

## concurrency & mutability

- `SymbolTable` owns an `Arc<TypeArena>` reference (read-only) so type ids remain valid without cloning.
- Mutation (ingestion, builder) requires `&mut SymbolTable`. For read-mostly scenarios, we expose snapshot iterators guarded by `RwLock` or `parking_lot::RwLock`, but the initial version can stay single-threaded until ingestion parallels are needed.

## performance tactics

- Use `rustc_hash::FxHashMap` or `ahash::AHashMap` for indices.
- Keep `SymbolRecord` <= 128 bytes by storing `Option<TypeId>` as `TypeId` + `bool` and compressing enums into `u16` bitfields.
- Store strings via the same string pool as the type system (shared interners) to avoid duplication across labels/sections.
- Provide `SymbolHandle` -> `&SymbolRecord` lookups via `#[inline]` functions to keep hot loops branchless.
- Precompute trust level (min across sources) once after ingestion rather than recomputing each query.

## interactions with types & memory

- Each symbol optionally references a `TypeId`; type updates (e.g., dynamic arrays resizing) only happen through arena APIs to avoid aliasing.
- Byte-order metadata lives on the symbol so readers know how to interpret memory ranges when the type itself is ambiguous.
- The symbol module coordinates with `soc::prog::types::builder` to automatically assign types to function labels (subroutines) and to auto-generate pointer types when calibration metadata only specifies sizes.

## future extensions

- **Relocation-aware views** – track per-segment offsets so loaders can adjust addresses when binaries are rebased.
- **Delta snapshots** – keep change logs for live calibration editing sessions.
- **Persistent caches** – serialize the table (symbols + indices) to disk to avoid rebuilding on every run.
- **Parallel ingestion** – once loaders are refactored into tasks, the table can accept batches via lock-free append buffers.

With this architecture, the Rust symbol subsystem remains closely aligned with the legacy .NET behavior while leveraging modern data layouts and builder ergonomics for high performance and expressivity.
