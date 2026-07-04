use super::OutputField;

#[derive(Clone, Copy)]
pub(crate) struct FieldBag<'a> {
    fields: &'a [OutputField],
}

impl<'a> FieldBag<'a> {
    pub(crate) const fn new(fields: &'a [OutputField]) -> Self {
        Self { fields }
    }

    pub(crate) fn value(&self, key: &str) -> Option<&'a str> {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.as_str())
    }

    pub(crate) fn is(&self, key: &str, value: &str) -> bool {
        self.value(key)
            .is_some_and(|field_value| field_value == value)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &'a OutputField> {
        self.fields.iter()
    }
}

pub(crate) fn format_field_value(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | ','))
    {
        return value.to_owned();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}
