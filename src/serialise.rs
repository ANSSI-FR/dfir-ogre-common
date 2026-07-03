use crate::{escape_special_chars, format_csv::CSV_DELIMITER, value::escape_special_chars_csv};

pub(crate) struct JsonObjectWriter<'a> {
    buffer: &'a mut String,
    fields: usize,
}

impl<'a> JsonObjectWriter<'a> {
    pub(crate) fn new(buffer: &'a mut String) -> Self {
        buffer.push('{');
        Self { buffer, fields: 0 }
    }

    pub(crate) fn field_raw<F>(&mut self, name: &str, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        self.write_field_prefix(name);
        write_value(self.buffer);
    }

    pub(crate) fn field_if<F>(&mut self, name: &str, should_emit: bool, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        if should_emit {
            self.field_raw(name, write_value);
        }
    }

    pub(crate) fn finish(self) {
        self.buffer.push('}');
    }

    fn write_field_prefix(&mut self, name: &str) {
        if self.fields != 0 {
            self.buffer.push(',');
        }
        self.buffer.push('"');
        self.buffer.push_str(&escape_special_chars(name));
        self.buffer.push_str("\":");
        self.fields += 1;
    }
}

pub(crate) struct JsonArrayWriter<'a> {
    buffer: &'a mut String,
    elements: usize,
}

impl<'a> JsonArrayWriter<'a> {
    pub(crate) fn new(buffer: &'a mut String) -> Self {
        buffer.push('[');
        Self {
            buffer,
            elements: 0,
        }
    }

    pub(crate) fn element_raw<F>(&mut self, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        if self.elements != 0 {
            self.buffer.push(',');
        }
        self.elements += 1;
        write_value(self.buffer);
    }

    pub(crate) fn element_if<F>(&mut self, should_emit: bool, write_value: F)
    where
        F: FnOnce(&mut String),
    {
        if should_emit {
            self.element_raw(write_value);
        }
    }

    pub(crate) fn finish(self) {
        self.buffer.push(']');
    }
}

pub(crate) struct CsvRowWriter<'a> {
    buffer: &'a mut String,
    fields: usize,
}

impl<'a> CsvRowWriter<'a> {
    pub(crate) fn new(buffer: &'a mut String) -> Self {
        Self { buffer, fields: 0 }
    }

    pub(crate) fn raw_field(&mut self, value: &str) {
        self.write_delimiter();
        self.buffer.push_str(value);
    }

    pub(crate) fn quoted_field(&mut self, value: &str) {
        self.write_delimiter();
        self.buffer.push('"');
        self.buffer.push_str(&escape_special_chars_csv(value));
        self.buffer.push('"');
    }

    pub(crate) fn optional_quoted_field(&mut self, value: Option<&str>) {
        match value {
            Some(value) => self.quoted_field(value),
            None => self.empty_field(),
        }
    }

    pub(crate) fn empty_field(&mut self) {
        self.write_delimiter();
    }

    fn write_delimiter(&mut self) {
        if self.fields != 0 {
            self.buffer.push(CSV_DELIMITER);
        }
        self.fields += 1;
    }
}
