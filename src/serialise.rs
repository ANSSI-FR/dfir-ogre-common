use crate::{
    Error, escape_special_chars, format_csv::CSV_DELIMITER, value::escape_special_chars_csv,
};

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

    pub(crate) fn try_field_raw<F>(&mut self, name: &str, write_value: F) -> Result<(), Error>
    where
        F: FnOnce(&mut String) -> Result<(), Error>,
    {
        let buffer_len = self.buffer.len();
        let fields = self.fields;
        self.write_field_prefix(name);
        if let Err(error) = write_value(self.buffer) {
            self.buffer.truncate(buffer_len);
            self.fields = fields;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn string_field(&mut self, name: &str, value: &str) {
        self.field_raw(name, |buffer| {
            buffer.push('"');
            buffer.push_str(&escape_special_chars(value));
            buffer.push('"');
        });
    }

    pub(crate) fn optional_string_field(&mut self, name: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.string_field(name, value);
        }
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

    pub(crate) fn try_element_raw<F>(&mut self, write_value: F) -> Result<(), Error>
    where
        F: FnOnce(&mut String) -> Result<(), Error>,
    {
        let buffer_len = self.buffer.len();
        let elements = self.elements;
        if self.elements != 0 {
            self.buffer.push(',');
        }
        self.elements += 1;
        if let Err(error) = write_value(self.buffer) {
            self.buffer.truncate(buffer_len);
            self.elements = elements;
            return Err(error);
        }
        Ok(())
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

    pub(crate) fn try_raw_field<F>(&mut self, write_value: F) -> Result<(), Error>
    where
        F: FnOnce(&mut String) -> Result<(), Error>,
    {
        let buffer_len = self.buffer.len();
        let fields = self.fields;
        self.write_delimiter();
        if let Err(error) = write_value(self.buffer) {
            self.buffer.truncate(buffer_len);
            self.fields = fields;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn try_quoted_field<F>(&mut self, write_value: F) -> Result<(), Error>
    where
        F: FnOnce(&mut String) -> Result<(), Error>,
    {
        let buffer_len = self.buffer.len();
        let fields = self.fields;
        let mut field_buffer = String::new();
        if let Err(error) = write_value(&mut field_buffer) {
            self.buffer.truncate(buffer_len);
            self.fields = fields;
            return Err(error);
        }
        self.quoted_field(&field_buffer);
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn json_object_writer_writes_strings_and_skips_missing_optionals() {
        let mut buffer = String::new();
        let mut object = JsonObjectWriter::new(&mut buffer);

        object.string_field("bad\"key", "value\"with\\chars\n");
        object.optional_string_field("missing", None);
        object.optional_string_field("present", Some("kept"));
        object.finish();

        let parsed: serde_json::Value = serde_json::from_str(&buffer).unwrap();
        assert_eq!(parsed["bad\"key"], "value\"with\\chars\n");
        assert_eq!(parsed["present"], "kept");
        assert!(parsed.get("missing").is_none());
    }

    #[test]
    fn json_object_writer_rolls_back_failed_field() {
        let mut buffer = String::from("prefix");
        let result = {
            let mut object = JsonObjectWriter::new(&mut buffer);
            let result = object.try_field_raw("bad", |buffer| {
                buffer.push_str("\"partial\"");
                Err(Error::PluginError("boom".to_owned()))
            });
            object.string_field("ok", "value");
            object.finish();
            result
        };

        assert!(matches!(result, Err(Error::PluginError(_))));
        assert_eq!(buffer, r#"prefix{"ok":"value"}"#);
    }

    #[test]
    fn json_array_writer_rolls_back_failed_element() {
        let mut buffer = String::from("prefix");
        let result = {
            let mut array = JsonArrayWriter::new(&mut buffer);
            array
                .try_element_raw(|buffer| {
                    buffer.push('1');
                    Ok(())
                })
                .unwrap();
            let result = array.try_element_raw(|buffer| {
                buffer.push_str("partial");
                Err(Error::PluginError("boom".to_owned()))
            });
            array
                .try_element_raw(|buffer| {
                    buffer.push('2');
                    Ok(())
                })
                .unwrap();
            array.finish();
            result
        };

        assert!(matches!(result, Err(Error::PluginError(_))));
        assert_eq!(buffer, "prefix[1,2]");
    }

    #[test]
    fn csv_row_writer_quotes_and_escapes_fields() {
        let mut buffer = String::new();
        let mut row = CsvRowWriter::new(&mut buffer);

        row.quoted_field("a\"b");
        row.optional_quoted_field(None);
        row.optional_quoted_field(Some("c"));

        assert_eq!(buffer, r#""a""b";;"c""#);
    }

    #[test]
    fn csv_row_writer_try_quoted_field_quotes_and_escapes_generated_content() {
        let mut buffer = String::new();
        let mut row = CsvRowWriter::new(&mut buffer);

        row.quoted_field("first");
        row.try_quoted_field(|field| {
            field.push_str("hello;\"world\"\nnext");
            Ok(())
        })
        .unwrap();
        row.quoted_field("last");

        assert_eq!(buffer, "\"first\";\"hello;\"\"world\"\"\nnext\";\"last\"");
    }

    #[test]
    fn csv_row_writer_try_quoted_field_rolls_back_failed_content() {
        let mut buffer = String::new();
        let result = {
            let mut row = CsvRowWriter::new(&mut buffer);
            row.quoted_field("first");
            let result = row.try_quoted_field(|field| {
                field.push_str("partial;\"payload\"");
                Err(Error::PluginError("boom".to_owned()))
            });
            row.quoted_field("second");
            result
        };

        assert!(matches!(result, Err(Error::PluginError(_))));
        assert_eq!(buffer, "\"first\";\"second\"");
    }

    #[test]
    fn csv_row_writer_rolls_back_failed_raw_field() {
        let mut buffer = String::new();
        let result = {
            let mut row = CsvRowWriter::new(&mut buffer);
            row.quoted_field("first");
            let result = row.try_raw_field(|buffer| {
                buffer.push_str("partial");
                Err(Error::PluginError("boom".to_owned()))
            });
            row.quoted_field("second");
            result
        };

        assert!(matches!(result, Err(Error::PluginError(_))));
        assert_eq!(buffer, r#""first";"second""#);
    }
}
