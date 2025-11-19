//! Runtime representation of a validated ISA along with helpers for disassembly and semantics.

use std::collections::BTreeMap;

use crate::soc::device::endianness::Endianness;
use crate::soc::prog::types::{BitFieldSegment, BitFieldSpec, TypeId};

use super::ast::{
    FormDecl,
    InstructionDecl,
    IsaItem,
    IsaSpecification,
    MaskSelector,
    SpaceAttribute,
    SpaceDecl,
    SpaceKind,
    SpaceMember,
    SubFieldOp,
};
use super::error::IsaError;
use super::semantics::SemanticBlock;

#[derive(Debug, Clone)]
pub struct MachineDescription {
    pub instructions: Vec<Instruction>,
    pub spaces: BTreeMap<String, SpaceInfo>,
    patterns: Vec<InstructionPattern>,
    word_bits: Option<u32>,
    endianness: Endianness,
}

impl Default for MachineDescription {
    fn default() -> Self {
        Self {
            instructions: Vec::new(),
            spaces: BTreeMap::new(),
            patterns: Vec::new(),
            word_bits: None,
            endianness: Endianness::Big,
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
        let mut instructions = Vec::new();
        for doc in docs {
            for item in doc.items {
                match item {
                    IsaItem::Space(space) => spaces.push(space),
                    IsaItem::SpaceMember(member) => match member.member {
                        SpaceMember::Form(form) => forms.push(form),
                        SpaceMember::Instruction(instr) => instructions.push(instr),
                        _ => {}
                    },
                    IsaItem::Instruction(instr) => instructions.push(instr),
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
        machine.build_patterns()?;
        Ok(machine)
    }

    /// Disassembles machine words assuming an implicit base address of zero.
    pub fn disassemble(&self, bytes: &[u8]) -> Vec<Disassembly> {
        self.disassemble_from(bytes, 0)
    }

    /// Disassembles machine words and annotates them with `base_address` offsets.
    pub fn disassemble_from(&self, bytes: &[u8], base_address: u64) -> Vec<Disassembly> {
        let Some(word_bits) = self.word_bits else {
            return Vec::new();
        };
        let word_bytes = (word_bits / 8) as usize;
        if word_bytes == 0 || bytes.len() < word_bytes {
            return Vec::new();
        }

        let mask = if word_bits == 64 {
            u64::MAX
        } else {
            (1u64 << word_bits) - 1
        };

        bytes
            .chunks(word_bytes)
            .enumerate()
            .filter(|(_, chunk)| chunk.len() == word_bytes)
            .map(|(index, chunk)| {
                let bits = decode_word(chunk, self.endianness) & mask;
                let address = base_address + (index as u64) * word_bytes as u64;
                if let Some(pattern) = self.best_match(bits) {
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
                        operands: vec![format!("0x{bits:0width$X}", width = word_bytes * 2)],
                    }
                }
            })
            .collect()
    }

    fn best_match(&self, bits: u64) -> Option<&InstructionPattern> {
        self.patterns
            .iter()
            .filter(|pattern| bits & pattern.mask == pattern.value)
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
                        format_operand(field, value)
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
        let space = self
            .spaces
            .get_mut(&form.space)
            .ok_or_else(|| IsaError::Machine(format!(
                "form '{}' declared for unknown space '{}'",
                form.name, form.space
            )))?;
        if space.kind != SpaceKind::Logic {
            return Ok(());
        }
        space.add_form(form)
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

        if let Some(first) = patterns.first() {
            self.word_bits = Some(first.width_bits);
            self.endianness = first.endianness;
        }
        self.patterns = patterns;
        Ok(())
    }

    fn build_pattern(
        &self,
        idx: usize,
        instr: &Instruction,
    ) -> Result<Option<InstructionPattern>, IsaError> {
        let Some(mask_spec) = instr.mask.as_ref() else {
            return Ok(None);
        };
        let space = self
            .spaces
            .get(&instr.space)
            .ok_or_else(|| IsaError::Machine(format!(
                "instruction '{}' references unknown space '{}'",
                instr.name, instr.space
            )))?;
        if space.kind != SpaceKind::Logic {
            return Ok(None);
        }
        let word_bits = space.word_bits()?;
        ensure_byte_aligned(word_bits, &instr.name)?;

        let mut mask = 0u64;
        let mut value_bits = 0u64;
        for field in &mask_spec.fields {
            let spec = match &field.selector {
                MaskSelector::Field(name) => {
                    let form_name = instr.form.as_ref().ok_or_else(|| IsaError::Machine(format!(
                        "instruction '{}' uses mask field '{}' without a form",
                        instr.name, name
                    )))?;
                    let form = space.forms.get(form_name).ok_or_else(|| IsaError::Machine(
                        format!(
                            "instruction '{}' references undefined form '{}::{}'",
                            instr.name, space.name, form_name
                        ),
                    ))?;
                    form.subfield(name).ok_or_else(|| IsaError::Machine(format!(
                        "instruction '{}' references unknown field '{}' on form '{}::{}'",
                        instr.name, name, space.name, form_name
                    )))?.spec.clone()
                }
                MaskSelector::BitExpr(expr) => parse_bit_spec(word_bits, expr).map_err(|err| {
                    IsaError::Machine(format!(
                        "invalid bit expression '{expr}' in instruction '{}': {err}",
                        instr.name
                    ))
                })?,
            };
            let (field_mask, encoded) = encode_constant(&spec, field.value).map_err(|err| {
                IsaError::Machine(format!(
                    "mask literal for instruction '{}' does not fit: {err}",
                    instr.name
                ))
            })?;
            if mask & field_mask != 0 && (value_bits & field_mask) != encoded {
                return Err(IsaError::Machine(format!(
                    "mask for instruction '{}' sets conflicting constraints",
                    instr.name
                )));
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
            width_bits: word_bits,
            endianness: space.endianness,
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
            info.push_field(FieldEncoding {
                name: sub.name,
                spec,
                operations: sub.operations,
            });
        }

        self.forms.insert(form.name, info);
        Ok(())
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
struct InstructionPattern {
    instruction_idx: usize,
    space: String,
    form: Option<String>,
    width_bits: u32,
    endianness: Endianness,
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
}

impl FieldEncoding {
    fn is_function_only(&self) -> bool {
        !self
            .operations
            .iter()
            .any(|op| !op.kind.eq_ignore_ascii_case("func"))
    }
}

impl Instruction {
}

fn ensure_byte_aligned(word_bits: u32, instr: &str) -> Result<usize, IsaError> {
    if word_bits % 8 != 0 {
        return Err(IsaError::Machine(format!(
            "instruction '{}' width ({word_bits} bits) is not byte-aligned",
            instr
        )));
    }
    Ok((word_bits / 8) as usize)
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
    let encoded = spec.write_bits(0, value).map_err(BitFieldSpecParseError::SpecError)?;
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

fn format_operand(field: &FieldEncoding, value: u64) -> String {
    if field
        .operations
        .iter()
        .any(|op| op.kind.eq_ignore_ascii_case("reg"))
    {
        format!("r{value}")
    } else {
        format!("{value}")
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
    use crate::soc::isa::builder::{mask_field_selector, subfield_op, IsaBuilder};

    #[test]
    fn lifter_decodes_simple_logic_space() {
        let mut builder = IsaBuilder::new("lift.isa");
        builder.add_space(
            "test",
            SpaceKind::Logic,
            vec![SpaceAttribute::WordSize(8), SpaceAttribute::Endianness(Endianness::Big)],
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
        assert_eq!(entry.operands, vec!["r5".to_string()]);
        assert_eq!(entry.opcode, 0xA5);
    }
}
