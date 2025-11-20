//! Runtime representation of a validated ISA along with helpers for disassembly and semantics.

use std::collections::BTreeMap;

use crate::soc::device::endianness::Endianness;
use crate::soc::prog::types::{BitFieldSegment, BitFieldSpec, TypeId};

use super::ast::{
    FieldDecl, FieldIndexRange, FormDecl, HintComparator, HintDecl, InstructionDecl, IsaItem,
    IsaSpecification, MaskSelector, SpaceAttribute, SpaceDecl, SpaceKind, SpaceMember, SubFieldOp,
};
use super::error::IsaError;
use super::semantics::SemanticBlock;

#[derive(Debug, Clone)]
pub struct MachineDescription {
    pub instructions: Vec<Instruction>,
    pub spaces: BTreeMap<String, SpaceInfo>,
    patterns: Vec<InstructionPattern>,
    decode_spaces: Vec<LogicDecodeSpace>,
}

impl Default for MachineDescription {
    fn default() -> Self {
        Self {
            instructions: Vec::new(),
            spaces: BTreeMap::new(),
            patterns: Vec::new(),
            decode_spaces: Vec::new(),
        }
    }
}

impl MachineDescription {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_documents(docs: Vec<IsaSpecification>) -> Result<Self, IsaError> {
        let mut spaces = Vec::new();
        let mut forms = Vec::new();
        let mut fields = Vec::new();
        let mut instructions = Vec::new();
        let mut hints = Vec::new();
        for doc in docs {
            for item in doc.items {
                match item {
                    IsaItem::Space(space) => spaces.push(space),
                    IsaItem::SpaceMember(member) => match member.member {
                        SpaceMember::Form(form) => forms.push(form),
                        SpaceMember::Instruction(instr) => instructions.push(instr),
                        SpaceMember::Field(field) => fields.push(field),
                    },
                    IsaItem::Instruction(instr) => instructions.push(instr),
                    IsaItem::Hint(block) => hints.extend(block.entries),
                    _ => {}
                }
            }
        }

        let mut machine = MachineDescription::new();
        for space in spaces {
            machine.register_space(space);
        }
        for form in forms {
            machine.register_form(form)?;
        }
        for instr in instructions {
            machine.instructions.push(Instruction::from_decl(instr));
        }
        for field in fields {
            machine.register_field(field)?;
        }
        for hint in hints {
            machine.apply_hint(hint)?;
        }
        machine.build_patterns()?;
        machine.build_decode_spaces()?;

        Ok(machine)
    }

    /// Disassembles machine words assuming an implicit base address of zero.
    pub fn disassemble(&self, bytes: &[u8]) -> Vec<Disassembly> {
        self.disassemble_from(bytes, 0)
    }

    /// Disassembles machine words and annotates them with `base_address` offsets.
    pub fn disassemble_from(&self, bytes: &[u8], base_address: u64) -> Vec<Disassembly> {
        if self.decode_spaces.is_empty() {
            return Vec::new();
        }
        let mut cursor = 0usize;
        let mut address = base_address;
        let mut listing = Vec::new();

        while cursor < bytes.len() {
            let remaining = &bytes[cursor..];
            let Some(space) = self.select_space(remaining) else {
                break;
            };
            if remaining.len() < space.word_bytes {
                break;
            }
            let chunk = &remaining[..space.word_bytes];
            let bits = decode_word(chunk, space.endianness) & space.mask;
            let entry = if let Some(pattern) = self.best_match(&space.name, bits) {
                let instr = &self.instructions[pattern.instruction_idx];
                let operands = self.decode_operands(pattern, bits);
                Disassembly {
                    address,
                    opcode: bits,
                    mnemonic: instr.name.clone(),
                    operands,
                }
            } else {
                Disassembly {
                    address,
                    opcode: bits,
                    mnemonic: "unknown".into(),
                    operands: vec![format!("0x{bits:0width$X}", width = space.word_bytes * 2)],
                }
            };
            listing.push(entry);
            cursor += space.word_bytes;
            address += space.word_bytes as u64;
        }

        listing
    }

    fn select_space(&self, bytes: &[u8]) -> Option<&LogicDecodeSpace> {
        self.decode_spaces.iter().find(|space| {
            if bytes.len() < space.word_bytes {
                return false;
            }
            match &space.hint {
                Some(predicate) => predicate.evaluate(&bytes[..space.word_bytes], space.endianness),
                None => true,
            }
        })
    }

    fn best_match(&self, space: &str, bits: u64) -> Option<&InstructionPattern> {
        self.patterns
            .iter()
            .filter(|pattern| pattern.space == space && bits & pattern.mask == pattern.value)
            .max_by_key(|pattern| pattern.specificity)
    }

    fn decode_operands(&self, pattern: &InstructionPattern, bits: u64) -> Vec<String> {
        let Some(form_name) = pattern.form.as_ref() else {
            return Vec::new();
        };
        let Some(space) = self.spaces.get(&pattern.space) else {
            return Vec::new();
        };
        let Some(form) = space.forms.get(form_name) else {
            return Vec::new();
        };

        pattern
            .operand_names
            .iter()
            .map(|name| {
                form.subfield(name)
                    .map(|field| {
                        let (value, _) = field.spec.read_bits(bits);
                        self.format_operand(field, value)
                    })
                    .unwrap_or_else(|| format!("?{name}"))
            })
            .collect()
    }

    fn register_space(&mut self, space: SpaceDecl) {
        let info = SpaceInfo::from_decl(space);
        self.spaces.insert(info.name.clone(), info);
    }

    fn register_form(&mut self, form: FormDecl) -> Result<(), IsaError> {
        let space = self.spaces.get_mut(&form.space).ok_or_else(|| {
            IsaError::Machine(format!(
                "form '{}' declared for unknown space '{}'",
                form.name, form.space
            ))
        })?;
        if space.kind != SpaceKind::Logic {
            return Ok(());
        }
        space.add_form(form)
    }

    fn register_field(&mut self, field: FieldDecl) -> Result<(), IsaError> {
        let space = self.spaces.get_mut(&field.space).ok_or_else(|| {
            IsaError::Machine(format!(
                "field '{}' declared for unknown space '{}'",
                field.name, field.space
            ))
        })?;
        space.add_register_field(field);
        Ok(())
    }

    fn apply_hint(&mut self, hint: HintDecl) -> Result<(), IsaError> {
        let space = self.spaces.get_mut(&hint.space).ok_or_else(|| {
            IsaError::Machine(format!(":hint references unknown space '{}'", hint.space))
        })?;
        if space.kind != SpaceKind::Logic {
            return Err(IsaError::Machine(format!(
                ":hint entries can only target logic spaces (got '{}')",
                hint.space
            )));
        }
        if space.hint.is_some() {
            return Err(IsaError::Machine(format!(
                "logic space '{}' already has a hint predicate",
                hint.space
            )));
        }
        space.hint = Some(hint);
        Ok(())
    }

    fn build_patterns(&mut self) -> Result<(), IsaError> {
        let mut patterns = Vec::new();
        for (idx, instr) in self.instructions.iter().enumerate() {
            if instr.mask.is_none() {
                continue;
            }
            if let Some(pattern) = self.build_pattern(idx, instr)? {
                patterns.push(pattern);
            }
        }
        self.patterns = patterns;
        Ok(())
    }

    fn build_decode_spaces(&mut self) -> Result<(), IsaError> {
        let mut spaces = Vec::new();
        for info in self.spaces.values() {
            if info.kind != SpaceKind::Logic {
                continue;
            }
            let word_bits = info.word_bits()?;
            let word_bytes = ensure_byte_aligned(word_bits, &info.name)?;
            let mask = mask_for_bits(word_bits);
            let hint = if let Some(hint) = &info.hint {
                Some(self.build_hint_predicate(word_bits, hint, &info.name)?)
            } else {
                None
            };
            spaces.push(LogicDecodeSpace {
                name: info.name.clone(),
                word_bits,
                word_bytes,
                mask,
                endianness: info.endianness,
                hint,
            });
        }

        if spaces.is_empty() {
            return Err(IsaError::Machine("no logic spaces defined".into()));
        }

        spaces.sort_by(|a, b| {
            a.word_bits
                .cmp(&b.word_bits)
                .then_with(|| a.name.cmp(&b.name))
        });
        self.decode_spaces = spaces;
        Ok(())
    }

    fn build_hint_predicate(
        &self,
        word_bits: u32,
        hint: &HintDecl,
        space: &str,
    ) -> Result<HintPredicate, IsaError> {
        let spec = parse_bit_spec(word_bits, &hint.selector).map_err(|err| {
            IsaError::Machine(format!(
                "invalid hint selector '{}' for space '{}': {err}",
                hint.selector, space
            ))
        })?;
        Ok(HintPredicate {
            spec,
            comparator: hint.comparator,
            expected: hint.value,
        })
    }

    fn format_operand(&self, field: &FieldEncoding, value: u64) -> String {
        if let Some(binding) = &field.register {
            if let Some(space) = self.spaces.get(&binding.space)
                && let Some(register) = space.registers.get(&binding.field)
            {
                return register.format(value);
            }
            return format!("{}{}", binding.field, value);
        }

        if field
            .operations
            .iter()
            .any(|op| op.kind.eq_ignore_ascii_case("reg"))
        {
            return format!("r{value}");
        }

        format!("{value}")
    }

    fn build_pattern(
        &self,
        idx: usize,
        instr: &Instruction,
    ) -> Result<Option<InstructionPattern>, IsaError> {
        let Some(mask_spec) = instr.mask.as_ref() else {
            return Ok(None);
        };
        let space = self.spaces.get(&instr.space).ok_or_else(|| {
            IsaError::Machine(format!(
                "instruction '{}' references unknown space '{}'",
                instr.name, instr.space
            ))
        })?;
        if space.kind != SpaceKind::Logic {
            return Ok(None);
        }
        let word_bits = space.word_bits()?;
        ensure_byte_aligned(word_bits, &instr.name)?;

        let mut mask = 0u64;
        let mut value_bits = 0u64;
        for field in &mask_spec.fields {
            let spec =
                match &field.selector {
                    MaskSelector::Field(name) => {
                        let form_name = instr.form.as_ref().ok_or_else(|| {
                            IsaError::Machine(format!(
                                "instruction '{}' uses mask field '{}' without a form",
                                instr.name, name
                            ))
                        })?;
                        let form = space.forms.get(form_name).ok_or_else(|| {
                            IsaError::Machine(format!(
                                "instruction '{}' references undefined form '{}::{}'",
                                instr.name, space.name, form_name
                            ))
                        })?;
                        form.subfield(name).ok_or_else(|| IsaError::Machine(format!(
                        "instruction '{}' references unknown field '{}' on form '{}::{}'",
                        instr.name, name, space.name, form_name
                    )))?.spec.clone()
                    }
                    MaskSelector::BitExpr(expr) => {
                        parse_bit_spec(word_bits, expr).map_err(|err| {
                            IsaError::Machine(format!(
                                "invalid bit expression '{expr}' in instruction '{}': {err}",
                                instr.name
                            ))
                        })?
                    }
                };
            let (field_mask, encoded) = encode_constant(&spec, field.value).map_err(|err| {
                IsaError::Machine(format!(
                    "mask literal for instruction '{}' does not fit: {err}",
                    instr.name
                ))
            })?;
            let overlap = mask & field_mask;
            if overlap != 0 {
                let previous = value_bits & field_mask;
                if previous != (encoded & field_mask) {
                    eprintln!(
                        "warning: instruction '{}' mask selector '{:?}' overrides previously set bits; treating as alias",
                        instr.name, field.selector
                    );
                }
            }
            mask |= field_mask;
            value_bits = (value_bits & !field_mask) | encoded;
        }

        let operand_names = if !instr.operands.is_empty() {
            instr.operands.clone()
        } else {
            instr
                .form
                .as_ref()
                .and_then(|form_name| space.forms.get(form_name))
                .map(|form| form.operand_order.clone())
                .unwrap_or_default()
        };

        Ok(Some(InstructionPattern {
            instruction_idx: idx,
            space: instr.space.clone(),
            form: instr.form.clone(),
            mask,
            value: value_bits,
            operand_names,
            specificity: mask.count_ones(),
        }))
    }
}

#[derive(Debug, Clone)]
pub struct SpaceInfo {
    pub name: String,
    pub kind: SpaceKind,
    pub size_bits: Option<u32>,
    pub endianness: Endianness,
    pub forms: BTreeMap<String, FormInfo>,
    pub registers: BTreeMap<String, RegisterInfo>,
    pub hint: Option<HintDecl>,
}

impl SpaceInfo {
    fn from_decl(space: SpaceDecl) -> Self {
        let mut size_bits = None;
        let mut endianness = Endianness::Big;
        for attr in &space.attributes {
            match attr {
                SpaceAttribute::WordSize(bits) => size_bits = Some(*bits),
                SpaceAttribute::Endianness(value) => endianness = *value,
                _ => {}
            }
        }
        Self {
            name: space.name,
            kind: space.kind,
            size_bits,
            endianness,
            forms: BTreeMap::new(),
            registers: BTreeMap::new(),
            hint: None,
        }
    }

    fn word_bits(&self) -> Result<u32, IsaError> {
        self.size_bits.ok_or_else(|| {
            IsaError::Machine(format!(
                "logic space '{}' missing required word size attribute",
                self.name
            ))
        })
    }

    fn add_form(&mut self, form: FormDecl) -> Result<(), IsaError> {
        let word_bits = self.word_bits()?;
        let mut info = if let Some(parent) = &form.parent {
            self.forms.get(parent).cloned().ok_or_else(|| {
                IsaError::Machine(format!(
                    "form '{}' inherits from undefined form '{}::{}'",
                    form.name, self.name, parent
                ))
            })?
        } else {
            FormInfo::new(form.name.clone())
        };

        for sub in form.subfields {
            if info.contains(&sub.name) {
                return Err(IsaError::Machine(format!(
                    "form '{}::{}' redeclares subfield '{}'",
                    self.name, form.name, sub.name
                )));
            }
            let spec = parse_bit_spec(word_bits, &sub.bit_spec).map_err(|err| {
                IsaError::Machine(format!(
                    "invalid bit spec '{}' on field '{}::{}::{}': {err}",
                    sub.bit_spec, self.name, form.name, sub.name
                ))
            })?;
            let register = derive_register_binding(&sub.operations);
            info.push_field(FieldEncoding {
                name: sub.name,
                spec,
                operations: sub.operations,
                register,
            });
        }

        self.forms.insert(form.name, info);
        Ok(())
    }

    fn add_register_field(&mut self, field: FieldDecl) {
        if self.kind != SpaceKind::Register {
            return;
        }
        let info = RegisterInfo::from_decl(field);
        self.registers.insert(info.name.clone(), info);
    }
}

#[derive(Debug, Clone)]
pub struct RegisterInfo {
    pub name: String,
    pub range: Option<FieldIndexRange>,
}

impl RegisterInfo {
    fn from_decl(decl: FieldDecl) -> Self {
        Self {
            name: decl.name,
            range: decl.range,
        }
    }

    fn format(&self, value: u64) -> String {
        if self.range.is_some() {
            format!("{}{}", self.name, value)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub space: String,
    pub name: String,
    pub form: Option<String>,
    pub description: Option<String>,
    pub operands: Vec<String>,
    pub mask: Option<InstructionMask>,
    pub encoding: Option<BitFieldSpec>,
    pub semantics: Option<SemanticBlock>,
}

impl Instruction {
    pub fn from_decl(decl: InstructionDecl) -> Self {
        Self {
            space: decl.space,
            name: decl.name,
            form: decl.form,
            description: decl.description,
            operands: decl.operands,
            mask: decl.mask.map(|mask| InstructionMask {
                fields: mask.fields,
            }),
            encoding: decl.encoding,
            semantics: decl.semantics,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstructionMask {
    pub fields: Vec<super::ast::MaskField>,
}

#[derive(Debug, Clone)]
pub struct Disassembly {
    pub address: u64,
    pub opcode: u64,
    pub mnemonic: String,
    pub operands: Vec<String>,
}

#[derive(Debug, Clone)]
struct LogicDecodeSpace {
    name: String,
    word_bits: u32,
    word_bytes: usize,
    mask: u64,
    endianness: Endianness,
    hint: Option<HintPredicate>,
}

#[derive(Debug, Clone)]
struct HintPredicate {
    spec: BitFieldSpec,
    comparator: HintComparator,
    expected: u64,
}

impl HintPredicate {
    fn evaluate(&self, bytes: &[u8], endianness: Endianness) -> bool {
        let bits = decode_word(bytes, endianness);
        let (value, _) = self.spec.read_bits(bits);
        match self.comparator {
            HintComparator::Equals => value == self.expected,
            HintComparator::NotEquals => value != self.expected,
        }
    }
}

#[derive(Debug, Clone)]
struct InstructionPattern {
    instruction_idx: usize,
    space: String,
    form: Option<String>,
    mask: u64,
    value: u64,
    operand_names: Vec<String>,
    specificity: u32,
}

#[derive(Debug, Clone)]
pub struct FormInfo {
    fields: Vec<FieldEncoding>,
    field_index: BTreeMap<String, usize>,
    operand_order: Vec<String>,
}

impl FormInfo {
    fn new(_name: String) -> Self {
        Self {
            fields: Vec::new(),
            field_index: BTreeMap::new(),
            operand_order: Vec::new(),
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.field_index.contains_key(name)
    }

    fn push_field(&mut self, field: FieldEncoding) {
        if !field.is_function_only() {
            self.operand_order.push(field.name.clone());
        }
        self.field_index
            .insert(field.name.clone(), self.fields.len());
        self.fields.push(field);
    }

    fn subfield(&self, name: &str) -> Option<&FieldEncoding> {
        self.field_index
            .get(name)
            .and_then(|index| self.fields.get(*index))
    }
}

#[derive(Debug, Clone)]
pub struct FieldEncoding {
    pub name: String,
    pub spec: BitFieldSpec,
    pub operations: Vec<SubFieldOp>,
    pub register: Option<RegisterBinding>,
}

impl FieldEncoding {
    fn is_function_only(&self) -> bool {
        !self
            .operations
            .iter()
            .any(|op| !op.kind.eq_ignore_ascii_case("func"))
    }
}

fn derive_register_binding(ops: &[SubFieldOp]) -> Option<RegisterBinding> {
    ops.iter().find_map(parse_register_op)
}

fn parse_register_op(op: &SubFieldOp) -> Option<RegisterBinding> {
    if let Some(binding) = parse_context_style_register(op) {
        return Some(binding);
    }
    if op.kind.eq_ignore_ascii_case("reg") {
        if let Some(field) = &op.subtype {
            return Some(RegisterBinding {
                space: "reg".into(),
                field: field.clone(),
            });
        }
    }
    None
}

fn parse_context_style_register(op: &SubFieldOp) -> Option<RegisterBinding> {
    if !op.kind.starts_with('$') {
        return None;
    }
    let mut segments: Vec<&str> = op.kind.split("::").collect();
    if segments.len() < 2 {
        return None;
    }
    let space = segments.remove(0).trim_start_matches('$');
    let field = segments.remove(0);
    if space.is_empty() || field.is_empty() {
        return None;
    }
    Some(RegisterBinding {
        space: space.to_string(),
        field: field.to_string(),
    })
}

#[derive(Debug, Clone)]
pub struct RegisterBinding {
    pub space: String,
    pub field: String,
}

impl Instruction {}

fn ensure_byte_aligned(word_bits: u32, instr: &str) -> Result<usize, IsaError> {
    if word_bits % 8 != 0 {
        return Err(IsaError::Machine(format!(
            "instruction '{}' width ({word_bits} bits) is not byte-aligned",
            instr
        )));
    }
    Ok((word_bits / 8) as usize)
}

fn mask_for_bits(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn parse_bit_spec(word_bits: u32, spec: &str) -> Result<BitFieldSpec, BitFieldSpecParseError> {
    let container = u16::try_from(word_bits).map_err(|_| BitFieldSpecParseError::TooWide)?;
    BitFieldSpec::from_spec_str(TypeId::from_index(0), container, spec)
        .map_err(BitFieldSpecParseError::SpecError)
}

fn encode_constant(spec: &BitFieldSpec, value: u64) -> Result<(u64, u64), BitFieldSpecParseError> {
    let mask = spec
        .segments
        .iter()
        .fold(0u64, |acc, segment| match segment {
            BitFieldSegment::Slice(slice) => acc | slice.mask,
            BitFieldSegment::Literal { .. } => acc,
        });
    let encoded = spec
        .write_bits(0, value)
        .map_err(BitFieldSpecParseError::SpecError)?;
    Ok((mask, encoded & mask))
}

fn decode_word(bytes: &[u8], endianness: Endianness) -> u64 {
    match endianness {
        Endianness::Little => bytes
            .iter()
            .enumerate()
            .fold(0u64, |acc, (idx, byte)| acc | ((*byte as u64) << (idx * 8))),
        Endianness::Big => bytes
            .iter()
            .fold(0u64, |acc, byte| (acc << 8) | (*byte as u64)),
    }
}

#[derive(Debug)]
enum BitFieldSpecParseError {
    TooWide,
    SpecError(crate::soc::prog::types::bitfield::BitFieldError),
}

impl std::fmt::Display for BitFieldSpecParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BitFieldSpecParseError::TooWide => write!(f, "bit spec exceeds 64-bit container"),
            BitFieldSpecParseError::SpecError(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BitFieldSpecParseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soc::isa::ast::{SpaceAttribute, SpaceKind, SubFieldDecl};
    use crate::soc::isa::builder::{IsaBuilder, mask_field_selector, subfield_op};

    #[test]
    fn lifter_decodes_simple_logic_space() {
        let mut builder = IsaBuilder::new("lift.isa");
        builder.add_space(
            "test",
            SpaceKind::Logic,
            vec![
                SpaceAttribute::WordSize(8),
                SpaceAttribute::Endianness(Endianness::Big),
            ],
        );
        builder.add_form(
            "test",
            "BASE",
            None,
            vec![
                SubFieldDecl {
                    name: "OPC".into(),
                    bit_spec: "@(0..3)".into(),
                    operations: vec![subfield_op("func", None::<&str>)],
                    description: None,
                },
                SubFieldDecl {
                    name: "DST".into(),
                    bit_spec: "@(4..7)".into(),
                    operations: vec![
                        subfield_op("target", None::<&str>),
                        subfield_op("reg", Some("GPR")),
                    ],
                    description: None,
                },
            ],
        );
        builder
            .instruction("test", "mov")
            .form("BASE")
            .mask_field(mask_field_selector("OPC"), 0xA)
            .finish();
        let doc = builder.build();
        let machine = MachineDescription::from_documents(vec![doc]).expect("machine");
        let bytes = [0xA5u8];
        let listing = machine.disassemble_from(&bytes, 0x1000);
        assert_eq!(listing.len(), 1);
        let entry = &listing[0];
        assert_eq!(entry.address, 0x1000);
        assert_eq!(entry.mnemonic, "mov");
        assert_eq!(entry.operands, vec!["GPR5".to_string()]);
        assert_eq!(entry.opcode, 0xA5);
    }

    #[test]
    fn xo_masks_overlap() {
        let xo = parse_bit_spec(32, "@(21..30)").expect("xo spec");
        let oe = parse_bit_spec(32, "@(21)").expect("oe spec");
        let (xo_mask, xo_bits) = encode_constant(&xo, 266).expect("xo encode");
        let (oe_mask, oe_bits) = encode_constant(&oe, 1).expect("oe encode");
        // PowerPC addo encodings set OE separately even though it's part of XO.
        // This asserts that our BitField encoding indeed produces conflicting bits,
        // justifying the override behavior in `build_pattern`.
        assert_eq!(xo_mask & oe_mask, oe_mask);
        assert_eq!(oe_bits, oe_mask);
        assert_eq!(xo_bits & oe_mask, 0);
    }
}
