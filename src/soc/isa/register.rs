use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub(crate) struct FieldInfo {
    pub(crate) subfields: HashSet<String>,
}

impl FieldInfo {
    pub(crate) fn new(subfields: HashSet<String>) -> Self {
        Self { subfields }
    }

    pub(crate) fn merge_subfields(&mut self, additional: impl IntoIterator<Item = String>) {
        self.subfields.extend(additional);
    }

    pub(crate) fn has_subfield(&self, name: &str) -> bool {
        self.subfields.contains(name)
    }
}

#[derive(Debug)]
pub(crate) enum FieldRegistrationError {
    DuplicateField { name: String },
    MissingBaseField { name: String },
    EmptySubfieldAppend { name: String },
}
