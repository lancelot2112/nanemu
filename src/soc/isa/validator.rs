//! Semantic validation for parsed ISA documents and the merged machine description.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::ast::{
    ContextReference, FieldDecl, IsaDocument, IsaItem, SpaceDecl, SpaceMember, SpaceMemberDecl,
};
use super::diagnostic::{DiagnosticLevel, DiagnosticPhase, IsaDiagnostic, SourceSpan};
use super::error::IsaError;
use super::machine::MachineDescription;

pub struct Validator {
    seen_spaces: BTreeSet<String>,
    parameters: BTreeMap<String, String>,
    space_states: BTreeMap<String, SpaceState>,
    diagnostics: Vec<IsaDiagnostic>,
}

impl Validator {
    pub fn new() -> Self {
        Self {
            seen_spaces: BTreeSet::new(),
            parameters: BTreeMap::new(),
            space_states: BTreeMap::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn validate(&mut self, docs: &[IsaDocument]) -> Result<(), IsaError> {
        for doc in docs {
            for item in &doc.items {
                match item {
                    IsaItem::Space(space) => self.validate_space(space),
                    IsaItem::Parameter(param) => {
                        self.parameters
                            .insert(param.name.clone(), format!("{:?}", param.value));
                    }
                    IsaItem::SpaceMember(member) => self.validate_space_member(member),
                    _ => {}
                }
            }
        }
        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(IsaError::Diagnostics {
                phase: DiagnosticPhase::Validation,
                diagnostics: std::mem::take(&mut self.diagnostics),
            })
        }
    }

    pub fn finalize_machine(&self, docs: Vec<IsaDocument>) -> Result<MachineDescription, IsaError> {
        MachineDescription::from_documents(docs)
    }

    fn validate_space(&mut self, space: &SpaceDecl) {
        if !self.seen_spaces.insert(space.name.clone()) {
            self.push_validation_diagnostic(
                "validation.duplicate-space",
                format!("space '{}' defined multiple times", space.name),
                Some(space.span.clone()),
            );
            return;
        }
        self.space_states
            .entry(space.name.clone())
            .or_insert_with(SpaceState::new);
    }

    fn validate_space_member(&mut self, member: &SpaceMemberDecl) {
        if let SpaceMember::Field(field) = &member.member {
            self.validate_field(field);
        }
    }

    fn validate_field(&mut self, field: &FieldDecl) {
        if let Some(reference) = &field.redirect {
            self.ensure_redirect_target_defined(field, reference);
        }

        let Some(state) = self.space_states.get_mut(&field.space) else {
            self.push_validation_diagnostic(
                "validation.unknown-space-field",
                format!(
                    "field '{}' declared in unknown space '{}'",
                    field.name, field.space
                ),
                Some(field.span.clone()),
            );
            return;
        };

        if let Err(FieldRegistrationError::DuplicateField) = register_field(state, field) {
            self.push_validation_diagnostic(
                "validation.duplicate-field",
                format!("field '{}' declared multiple times", field.name),
                Some(field.span.clone()),
            );
        }
    }

    fn push_validation_diagnostic(
        &mut self,
        code: &'static str,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) {
        self.diagnostics.push(IsaDiagnostic::new(
            DiagnosticPhase::Validation,
            DiagnosticLevel::Error,
            code,
            message,
            span,
        ));
    }
    fn ensure_redirect_target_defined(&mut self, field: &FieldDecl, reference: &ContextReference) {
        let (target_space, mut path) = resolve_reference_path(&field.space, reference);
        if path.is_empty() {
            self.push_validation_diagnostic(
                "validation.redirect.missing-field",
                "redirect requires a field name in its context reference",
                Some(field.span.clone()),
            );
            return;
        }
        let field_name = path.remove(0);
        let Some(space_state) = self.space_states.get(&target_space) else {
            self.push_validation_diagnostic(
                "validation.redirect.unknown-space",
                format!("redirect references undefined space '{}'", target_space),
                Some(field.span.clone()),
            );
            return;
        };
        let Some(field_info) = space_state.lookup_field(&field_name) else {
            self.push_validation_diagnostic(
                "validation.redirect.unknown-field",
                format!(
                    "redirect references undefined field '{}' in space '{}'",
                    field_name, target_space
                ),
                Some(field.span.clone()),
            );
            return;
        };
        if let Some(subfield_name) = path.get(0) {
            if !field_info.has_subfield(subfield_name) {
                self.push_validation_diagnostic(
                    "validation.redirect.unknown-subfield",
                    format!(
                        "redirect references undefined subfield '{}' on field '{}'",
                        subfield_name, field_name
                    ),
                    Some(field.span.clone()),
                );
                return;
            }
            if path.len() > 1 {
                self.push_validation_diagnostic(
                    "validation.redirect.depth",
                    "redirect context depth exceeds field::subfield",
                    Some(field.span.clone()),
                );
            }
        } else if path.len() > 0 {
            self.push_validation_diagnostic(
                "validation.redirect.depth",
                "redirect context depth exceeds field::subfield",
                Some(field.span.clone()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::isa::parse_str;
    use crate::soc::isa::diagnostic::DiagnosticPhase;
    use std::path::PathBuf;

    fn validate_src(source: &str) -> Result<(), IsaError> {
        let doc = parse_str(PathBuf::from("test.isa"), source)?;
        let mut validator = Validator::new();
        validator.validate(&[doc])
    }

    fn expect_validation_diag(err: IsaError, needle: &str) {
        match err {
            IsaError::Diagnostics {
                phase: DiagnosticPhase::Validation,
                diagnostics,
            } => {
                assert!(
                    diagnostics.iter().any(|diag| diag.message.contains(needle)),
                    "no diagnostic containing '{needle}': {diagnostics:?}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn redirect_requires_prior_definition_in_same_space() {
        let err = validate_src(
            ":space reg addr=32 word=64 type=register\n:reg alias redirect=PC\n:reg PC size=64",
        )
        .unwrap_err();
        expect_validation_diag(err, "undefined field 'PC'");
    }

    #[test]
    fn redirect_accepts_prior_definition() {
        validate_src(
            ":space reg addr=32 word=64 type=register\n:reg PC size=64\n:reg alias redirect=PC",
        )
        .expect("validation succeeds");
    }

    #[test]
    fn redirect_supports_cross_space_reference() {
        validate_src(
            ":space reg addr=32 word=64 type=register\n:reg PC size=64\n:space aux addr=32 word=64 type=register\n:aux backup redirect=$reg::PC",
        )
        .expect("cross space redirect succeeds");
    }

    #[test]
    fn redirect_errors_on_unknown_subfield() {
        let err = validate_src(
            ":space reg addr=32 word=64 type=register\n:reg PC size=64 subfields={\n    LSB @(0)\n}\n:reg alias redirect=PC::MSB",
        )
        .unwrap_err();
        expect_validation_diag(err, "undefined subfield 'MSB'");
    }

    #[test]
    fn validator_collects_multiple_errors() {
        let err = validate_src(
            ":space reg addr=32 word=64 type=register\n:reg alias redirect=PC\n:reg R0 size=64\n:reg R0 size=64",
        )
        .unwrap_err();
        match err {
            IsaError::Diagnostics {
                phase: DiagnosticPhase::Validation,
                diagnostics,
            } => {
                assert!(
                    diagnostics.len() >= 2,
                    "expected multiple diagnostics: {diagnostics:?}"
                );
                assert!(
                    diagnostics
                        .iter()
                        .any(|diag| diag.message.contains("undefined field 'PC'")),
                    "missing redirect diagnostic: {diagnostics:?}"
                );
                assert!(
                    diagnostics
                        .iter()
                        .any(|diag| diag.message.contains("field 'R0' declared multiple times")),
                    "missing duplicate field diagnostic: {diagnostics:?}"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}

#[derive(Default)]
struct SpaceState {
    fields: HashMap<String, FieldInfo>,
    ranges: Vec<RangedFieldInfo>,
}

impl SpaceState {
    fn new() -> Self {
        Self::default()
    }

    fn lookup_field(&self, name: &str) -> Option<FieldLookup<'_>> {
        if let Some(info) = self.fields.get(name) {
            return Some(FieldLookup::Direct(info));
        }
        for entry in &self.ranges {
            if entry.matches(name) {
                return Some(FieldLookup::Ranged(entry));
            }
        }
        None
    }
}

struct FieldInfo {
    subfields: HashSet<String>,
}

struct RangedFieldInfo {
    base: String,
    start: u32,
    end: u32,
    subfields: HashSet<String>,
}

enum FieldLookup<'a> {
    Direct(&'a FieldInfo),
    Ranged(&'a RangedFieldInfo),
}

impl<'a> FieldLookup<'a> {
    fn has_subfield(&self, name: &str) -> bool {
        match self {
            FieldLookup::Direct(info) => info.subfields.contains(name),
            FieldLookup::Ranged(info) => info.subfields.contains(name),
        }
    }
}

impl RangedFieldInfo {
    fn matches(&self, candidate: &str) -> bool {
        if !candidate.starts_with(&self.base) {
            return false;
        }
        let suffix = &candidate[self.base.len()..];
        if suffix.is_empty() {
            return false;
        }
        parse_index_suffix(suffix)
            .map(|index| index >= self.start && index <= self.end)
            .unwrap_or(false)
    }
}

fn resolve_reference_path(
    current_space: &str,
    reference: &ContextReference,
) -> (String, Vec<String>) {
    if let Some(first) = reference.segments.first() {
        if first.starts_with('$') {
            let space = first.trim_start_matches('$').to_string();
            let rest = reference.segments[1..].to_vec();
            return (space, rest);
        }
    }
    (current_space.to_string(), reference.segments.clone())
}

enum FieldRegistrationError {
    DuplicateField,
}

fn register_field(state: &mut SpaceState, field: &FieldDecl) -> Result<(), FieldRegistrationError> {
    let subfields: HashSet<String> = field.subfields.iter().map(|sub| sub.name.clone()).collect();
    if let Some(range) = &field.range {
        if state.ranges.iter().any(|entry| entry.base == field.name) {
            return Err(FieldRegistrationError::DuplicateField);
        }
        state.ranges.push(RangedFieldInfo {
            base: field.name.clone(),
            start: range.start,
            end: range.end,
            subfields,
        });
    } else {
        if state.fields.contains_key(&field.name) {
            return Err(FieldRegistrationError::DuplicateField);
        }
        state
            .fields
            .insert(field.name.clone(), FieldInfo { subfields });
    }
    Ok(())
}

fn parse_index_suffix(text: &str) -> Option<u32> {
    let cleaned: String = text.replace('_', "");
    if cleaned.is_empty() {
        return None;
    }
    if let Some(hex) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        u32::from_str_radix(bin, 2).ok()
    } else if let Some(oct) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        u32::from_str_radix(oct, 8).ok()
    } else {
        u32::from_str_radix(&cleaned, 10).ok()
    }
}
