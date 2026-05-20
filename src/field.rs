use crate::{
    Record, Value,
    date_util::{DateInputCodec, parse_date},
    errors::Error,
};

use indexmap::IndexMap;

use pyo3::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Defines parsing strategies for different data types.
/// it is used in data ingestion and transformation pipelines to specify how raw input should be interpreted and converted into structured `Value` types.
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub enum Parser {
    /// Ignores the input value entirely; useful for skipping fields.
    Ignore(),

    /// Parses the input as a signed 64-bit integer.
    Int(),

    /// Parses the input as an integer using the specified base (radix), e.g., 2 for binary, 16 for hexadecimal.
    IntRadix(u32),

    /// Parse the input as a signed 64-bit integer and convert it to hexadecimal string with configurable width for zero-padding.
    /// - width = 0: no padding, width > 0: pad with leading zeros
    IntToHex(u16),

    /// try each parsers in the list in insertion order and return the first successfull parse
    Dynamic(Vec<Parser>),

    /// Parses the input as a floating-point number (f64).
    Float(),

    /// Parses the input as a boolean value; Treats 'false', '0', 'no', 'n', '','off' as False, everything else is considered to be True
    Bool(),

    /// Treats the input as a raw string without any transformation.
    String(),

    /// Convert the input string to uppercase.
    StringToUpper(),

    /// Convert the input string to lowercase.
    StringToLower(),

    /// Parses the input as a date/time string using the provided date codec, which defines the expected format.
    DateTime(DateInputCodec),

    /// Splits the input string using the provided delimiter and returns the resulting parts as a list.
    Split(String),

    /// Uses a Python-defined parser function to process the input; allows for custom logic via Python code.
    Python(PyParser),

    /// extends the basic parser with user defined parser. This struct encapsulate the PaserExtensionTrait to be able to call it from python
    Extension(ParserExtension),
}
impl Parser {
    /// Applies the parsing logic defined by the `Parser` variant to the provided `input` string,
    /// and stores the resulting `Value` in the `output` map under the key `output_name`.
    /// If the input is empty, the function returns early without making any changes.
    ///
    /// # Arguments
    ///
    /// * `output_name` - The key under which the parsed value will be stored in the output map.
    /// * `input` - The raw string input to be parsed.
    /// * `output` - A mutable reference to a map where the parsed value will be inserted.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if parsing succeeds, or an `Error` if parsing fails (e.g., invalid number format or invalid date).
    ///
    pub fn parse(
        &self,
        output_name: &str,
        input: Option<&str>,
        output: &mut Record,
    ) -> Result<(), Error> {
        match &self {
            Parser::Python(py_parser) => {
                let input = match input {
                    Some(val) => val,
                    None => return Ok(()),
                };
                let mut record: Record = Python::attach(|py| {
                    match py_parser
                        .0
                        .bind(py)
                        .call_method("parse", (input, output_name), None)
                    {
                        Ok(parsed) => parsed.extract().map_err(Into::into),
                        Err(e) => Err(e),
                    }
                })?;
                output.extend(record.drain());
            }
            Parser::Extension(parser_extension) => {
                let input = match input {
                    Some(val) => val,
                    None => return Ok(()),
                };
                if let Some(tuple) = parser_extension.0.parse(input, output_name) {
                    for (key, value) in tuple {
                        output.insert(key, value);
                    }
                }
            }
            _ => match self.get_value(input) {
                Ok(value_opt) => {
                    if let Some(value) = value_opt {
                        output.insert(output_name.to_owned(), value);
                    }
                }
                Err(e) => {
                    return Err(Error::ParseField(output_name.to_owned(), e.to_string()));
                }
            },
        }
        Ok(())
    }

    /// Parses the input string into a `Value` based on the parser's configuration.
    ///
    /// This method handles various parser variants (e.g., `Int`, `Float`, `Bool`, `DateTime`, etc.)
    /// and returns the corresponding `Value` type.
    ///
    /// # Arguments
    /// * `input` - The input string to parse.
    ///
    pub fn get_value(&self, input: Option<&str>) -> Result<Option<Value>, Error> {
        let input = match input {
            Some(val) => val,
            None => return Ok(Some(Value::Null())),
        };
        let value = match &self {
            Parser::Ignore() | Parser::Python(_) | Parser::Extension(_) => None,
            Parser::Dynamic(parsers) => {
                let mut ret_value = None;
                for parser in parsers {
                    if let Ok(parsed_val) = parser.get_value(Some(input)) {
                        ret_value = parsed_val;
                        //returns the first sucessfull parsing
                        break;
                    }
                }
                ret_value
            }
            Parser::Int() => {
                let value = input.parse()?;
                Some(Value::Int(value))
            }
            Parser::IntRadix(radix) => {
                let value = if input.starts_with("0x") {
                    let inp = input.replacen("0x", "", 1);
                    i64::from_str_radix(&inp, *radix)?
                } else {
                    i64::from_str_radix(input, *radix)?
                };
                Some(Value::Int(value))
            }
            Parser::IntToHex(width) => {
                let value: i64 = input.parse()?;
                let hex_str = if *width > 0 {
                    format!("0x{value:0width$X}", width = *width as usize)
                } else {
                    format!("0x{value:X}")
                };
                Some(Value::String(hex_str))
            }
            Parser::Float() => {
                let value: f64 = input.parse()?;
                Some(Value::Float(value))
            }
            Parser::Bool() => {
                let value = input.to_lowercase();
                let bol = !matches!(value.as_str(), "false" | "0" | "n" | "" | "off" | "no");
                Some(Value::Bool(bol))
            }
            Parser::String() => Some(Value::String(input.to_owned())),
            Parser::StringToUpper() => Some(Value::String(input.to_owned().to_uppercase())),
            Parser::StringToLower() => Some(Value::String(input.to_owned().to_lowercase())),
            Parser::DateTime(codec) => {
                let date = parse_date(input, codec)?;
                Some(Value::Date(date))
            }
            Parser::Split(separator) => {
                let parts: Vec<Value> = input
                    .split(separator)
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                Some(Value::Array(parts))
            }
        };
        Ok(value)
    }

    /// Retrieves the list of field names that this parser will output.
    ///
    /// For `Parser::Python`, it calls the `output_fields_names` method on the underlying Python object.
    /// For all other parser variants, it returns an empty vector since they produce a single field.
    ///
    /// # Returns
    ///
    /// A vector of field names that will be produced by this parser.
    ///
    pub fn output_fields_names(&self) -> Vec<FieldName> {
        match &self {
            Parser::Python(python_parser) => {
                let fields_result: PyResult<Vec<FieldName>> = Python::attach(|py| {
                    match python_parser
                        .0
                        .bind(py)
                        .call_method("output_fields_names", (), None)
                    {
                        Ok(parsed) => parsed.extract(),
                        Err(e) => Err(e),
                    }
                });

                if let Ok(fields) = fields_result {
                    return fields;
                }
                vec![]
            }
            Parser::Extension(parser_ext) => parser_ext.0.output_fields_names(),
            _ => vec![],
        }
    }
}

/// Trait defining a parser extension that can be called from Python.
///
/// Implementors provide a human‑readable name, a parsing routine that
/// returns optional field/value pairs, and a method that reports the
/// field names the extension will output.
/// Trait defining a parser extension that can be called from Python.
///
pub trait ParserExtensionTrait {
    /// Returns the extension's name.
    fn name(&self) -> String;
    /// Parses `input` and produces an optional list of `(field_name, Value)` pairs.
    ///
    /// Returning `None` means the extension chose not to produce any output.
    fn parse(&self, input: &str, output_name: &str) -> Option<Vec<(String, Value)>>;
    /// Lists the field names that will be generated by this extension.
    fn output_fields_names(&self) -> Vec<FieldName>;
}

use core::fmt::Debug;
impl Debug for dyn ParserExtensionTrait + Sync + Send {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Rust extention: {}", self.name())
    }
}

///
/// Wrapper around the parser extension to be able to call it from python
///
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
/// Wrapper exposing a Rust parser extension to Python via pyo3.
///
/// The inner `Arc` enables shared ownership and thread‑safe access.
pub struct ParserExtension(pub Arc<dyn ParserExtensionTrait + Sync + Send>);

/// Wraps a Python objects for integration with Rust code via pyo3.
/// This is used to delegate parsing operations to Python implementations.
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
/// Holds a reference to a Python object that implements the expected parsing API.
///
/// Instances are created from a Python parser and then used by the Rust
/// `Parser::Python` variant.
pub struct PyParser(pub Arc<Py<PyAny>>);
#[pymethods]
impl PyParser {
    #[new]
    /// Creates a `PyParser` from a Python object that implements the expected parsing API.
    ///
    /// The supplied Python parser is stored inside an `Arc` so it can be shared
    /// safely across threads and used by the Rust `Parser::Python` variant.
    pub fn new(parser: Py<PyAny>) -> Self {
        Self(Arc::new(parser))
    }
}

/// Defines multi-input field parsing strategies.
/// This enum supports parsing logic that combines values from multiple input fields into a single output value.
/// Currently, only the `Join` variant is implemented, which concatenates multiple input strings
/// using a specified separator.
///
/// The `Join` variant offers two modes:
/// - With `avoid_duplication = true`: it attempts to avoid duplicate separators by checking
///   if the previous segment ends with the separator or the next begins with it.
/// - With `avoid_duplication = false`: it simply joins all values using the provided separator.
///
/// This is useful when combining fields such as paths, tags, or lists of values where
/// clean formatting is required.
///
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub enum MultiParser {
    /// A parser that joins multiple input strings into a single string.
    ///
    /// # Fields
    ///
    /// * `separator`: The string to insert between each input value.
    /// * `avoid_duplication`: If `true`, the parser will attempt to avoid duplicate separators
    ///   by checking for overlapping boundaries between values.
    ///
    Join(String, bool),
}

impl MultiParser {
    /// Parses multiple input strings and combines them into a single output string.
    ///
    /// This function takes a map of input field names to their string values, joins them using
    /// the configured separator, and stores the result in the output map under `output_name`.
    ///
    /// If the input map is empty, the function returns early without modifying the output.
    ///
    /// # Arguments
    ///
    /// * `input` - A map of field names to their string values to be combined.
    /// * `output_name` - The key under which the joined string will be stored in the output map.
    /// * `output` - A mutable reference to the output map where the result will be inserted.
    ///
    pub fn parse(
        &self,
        input: &IndexMap<String, Option<String>>,
        output_name: &str,
        output: &mut Record,
    ) -> Result<(), Error> {
        if input.is_empty() {
            return Ok(());
        }
        match &self {
            MultiParser::Join(separator, avoid_duplication) => {
                let mut joined = String::new();

                if *avoid_duplication {
                    for (_, value) in input {
                        let value = value.clone().unwrap_or("".to_string());
                        if !joined.is_empty() {
                            if joined.ends_with(separator) || value.starts_with(separator) {
                                joined.push_str(&value);
                            } else {
                                joined.push_str(separator);
                                joined.push_str(&value);
                            }
                        } else {
                            joined.push_str(&value);
                        }
                    }
                } else {
                    joined = input
                        .values()
                        .map(|value| value.clone().unwrap_or("".to_string()))
                        .collect::<Vec<String>>()
                        .join(separator);
                }
                output.insert(output_name.to_owned(), Value::String(joined));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
/// Metadata describing a field's mapping between input and output.
///
/// * `in_name` – name of the field in the source data.
/// * `out_name` – name used in the output; defaults to `in_name` when omitted.
/// * `qualifier` – optional qualifier that can be appended to `out_name`.
/// * `qualified_name` – cached version of `out_name:qualifier` when a
///   qualifier is present.
/// * `display_name` – a friendly label for UI or timeline displays.
/// * `description` – a longer textual explanation of the field's purpose.
pub struct FieldName {
    pub in_name: String,
    pub out_name: String,
    pub primary_key: bool,
    pub qualifier: Option<String>,
    pub qualified_name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[pymethods]
impl FieldName {
    ///
    /// # Arguments
    ///
    /// * `input_name` - The name of the field in the input data.
    /// * `output_name` - The desired name in the output data. If `None`, defaults to `input_name`.
    /// * `qualifier` - An optional qualifier to append to the output name for classification.
    /// * `description` - An optional description to document the field's purpose or meaning.
    ///
    #[new]
    #[pyo3(signature = (input_name, primary_key=false, output_name=None, qualifier=None,display_name=None, description=None))]
    pub fn new(
        input_name: String,
        primary_key: bool,
        output_name: Option<String>,
        qualifier: Option<String>,
        display_name: Option<String>,
        description: Option<String>,
    ) -> Self {
        let output_name = output_name.unwrap_or(input_name.clone());

        let qualified_name = qualifier
            .as_ref()
            .map(|qual| format!("{}:{}", &output_name, qual));

        FieldName {
            in_name: input_name,

            primary_key,
            out_name: output_name,
            qualifier,
            qualified_name,
            description,
            display_name,
        }
    }

    /// Returns the field name, optionally including the qualifier.
    ///
    /// If `with_qualifier` is `true` and a qualifier is present, the qualified name (e.g., `name:qual`) is returned.
    /// Otherwise, the base `out_name` is returned.
    ///
    /// # Arguments
    ///
    /// * `with_qualifier` - Whether to include the qualifier in the returned name.
    ///
    pub fn name(&self, with_qualifier: bool) -> &str {
        if with_qualifier && self.qualified_name.is_some() {
            self.qualified_name.as_deref().unwrap_or(&self.out_name)
        } else {
            &self.out_name
        }
    }

    #[inline]
    /// Returns the name of the field as it appears in the input data.
    pub fn input_name(&self) -> &str {
        &self.in_name
    }

    #[inline]
    /// Returns the name of the field as it is intended to appear in the output data.
    pub fn output_name(&self) -> &str {
        &self.out_name
    }

    #[inline]
    /// Returns a human-readable display name of the field.
    /// it is used in the timeline timestamp_meaning field
    /// If no description is provided, returns the output name as a fallback.
    pub fn display(&self) -> &str {
        if let Some(ref desc) = self.display_name {
            desc
        } else {
            &self.out_name
        }
    }

    #[inline]
    /// Returns a human-readable description of the field.
    ///
    /// If no description is provided, returns the output name as a fallback.
    pub fn describe(&self) -> &str {
        if let Some(ref desc) = self.description {
            desc
        } else {
            &self.out_name
        }
    }
}

/// This is used to represent array fields in the data model
/// It is a compatibility layer for the Pyo3 python binding which does not support boxed values
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
/// Compatibility wrapper for array fields when exposed to Python.
///
/// `ArrayField` stores a boxed `Field` because the pyo3 binding cannot
/// directly handle generic box types.
pub struct ArrayField(pub Box<Field>);
#[pymethods]
impl ArrayField {
    #[new]
    /// Constructs a new `ArrayField` wrapping the provided `Field`.
    ///
    /// This wrapper exists because the Pyo3 bindings cannot directly expose a
    /// generic boxed type. The `Field` is stored on the heap to satisfy the
    /// binding requirements.
    pub fn new(field: Field) -> Self {
        Self(Box::new(field))
    }
}

/// Represents different types of fields in the data model.
/// Contains three variants: simple fields, multi-fields, and object fields.
/// Each variant defines how data should be parsed and transformed.
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub enum Field {
    /// Maps a single input field to an output field using a parser.
    ///
    /// # Fields
    ///
    /// * `name`: The metadata for the field, including input/output names, qualifiers, and descriptions.
    /// * `parser`: The strategy used to parse the input value into a structured `Value`.
    ///
    Single {
        name: FieldName,
        parser: Parser,
        default_value: Option<String>,
    },

    /// Combines values from multiple input fields into a single output value.
    ///
    /// # Arguments
    ///
    /// *  instance of the field.
    Multi(MultiInputField),

    /// Represents an array field in the data model.
    /// This variant is used to handle collections of values, where each element
    /// is processed using the wrapped `Field` configuration.
    ///
    /// The `ArrayField` is a compatibility layer for Pyo3, which does not support
    /// boxed values directly.
    Array(ArrayField),

    /// An object field that represents a nested structure composed of multiple child fields.
    ///
    /// This allows for hierarchical data modeling where a single field contains a group of sub-fields,
    /// each with its own parsing logic and metadata.
    ///
    /// # Fields
    ///
    /// * `name`: The metadata for the object field, including input/output names and description.
    /// * `ignore`: ignore the content of the nested structure
    /// * `fields`: A list of child fields that make up the object. These are parsed recursively.
    ///
    #[pyo3(constructor = (name, fields, ignore=false))]
    Object {
        name: FieldName,
        fields: Vec<Field>,
        ignore: bool,
    },
}

impl Field {
    /// Returns the output name of the field, optionally including the qualifier.
    ///
    /// # Arguments
    ///
    /// * `with_qualifier` - If `true`, includes the qualifier in the output name (e.g., `name:qual`).
    ///
    pub fn name(&self, with_qualifier: bool) -> &str {
        match self {
            Field::Single {
                name,
                parser: _,
                default_value: _,
            } => name.name(with_qualifier),
            Field::Multi(f) => f.output_field.name(with_qualifier),
            Field::Object {
                name,
                fields: _,
                ignore: _,
            } => name.name(with_qualifier),
            Field::Array(field) => field.0.name(with_qualifier),
        }
    }

    /// Returns a human-readable description of the field.
    pub fn describe(&self) -> &str {
        match self {
            Field::Single {
                name: input_field,
                parser: _,
                default_value: _,
            } => input_field.describe(),
            Field::Multi(f) => f.output_field.describe(),
            Field::Object {
                name: input_field,
                fields: _,
                ignore: _,
            } => input_field.describe(),
            Field::Array(field) => field.0.describe(),
        }
    }

    /// Returns the base output name as defined in the field's metadata, without any qualifier.
    pub fn output_name(&self) -> &str {
        match self {
            Field::Single {
                name: input_field,
                parser: _,
                default_value: _,
            } => input_field.output_name(),
            Field::Multi(f) => f.output_field.output_name(),
            Field::Object {
                name: input_field,
                fields: _,
                ignore: _,
            } => input_field.output_name(),
            Field::Array(field) => field.0.output_name(),
        }
    }

    /// Returns a list of all input field names associated with this field.
    ///
    /// For a `Simple` field, returns the input name of the single source field.
    /// For a `Multi` field, recursively collects all input names from the child fields.
    /// For an `Object` field, returns the input name of the object itself.
    ///
    pub fn input_names(&self) -> Vec<String> {
        match self {
            Field::Single {
                name: input_field,
                parser: _,
                default_value: _,
            } => vec![input_field.input_name().to_owned()],
            Field::Multi(f) => {
                let mut v = vec![];
                for a in &f.input_fields {
                    v.append(&mut a.input_names().to_owned());
                }
                v
            }
            Field::Object {
                name: input_field,
                fields: _,
                ignore: _,
            } => vec![input_field.input_name().to_owned()],
            Field::Array(field) => field.0.input_names(),
        }
    }

    pub fn ignore(&self) -> bool {
        match self {
            Field::Single {
                name: _,
                parser,
                default_value: _,
            } => {
                if let Parser::Ignore() = parser {
                    true
                } else {
                    false
                }
            }
            Field::Multi(_) => false,
            Field::Array(_) => false,
            Field::Object {
                name: _,
                fields: _,
                ignore,
            } => *ignore,
        }
    }

    /// Parses input data based on the field's configuration and inserts the result into the output map.
    ///
    /// This method applies the appropriate parsing logic depending on the field type:
    /// - For `Simple` fields: uses the associated parser to convert the input string into a `Value`.
    /// - For `Multi` fields: delegates to `MultiParser` to combine multiple input fields.
    /// - For `Object` fields: does nothing, as object fields needs to be parsed recursively through their child fields.
    ///
    /// If the input string is empty, no parsing is performed.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the field being parsed (used for context in error messages or logging).
    /// * `input` - The raw input string to be parsed.
    /// * `output` - A mutable reference to the output map where parsed values will be inserted.
    ///
    pub fn parse(&self, name: &str, input: Option<&str>, output: &mut Record) -> Result<(), Error> {
        match self {
            Field::Single {
                name: input_field,
                parser,
                default_value,
            } => {
                //field has a default value
                if let Some(default_value) = default_value {
                    match input {
                        Some(val) => {
                            if val.is_empty() {
                                parser.parse(
                                    input_field.output_name(),
                                    Some(default_value),
                                    output,
                                )?
                            } else {
                                parser.parse(input_field.output_name(), input, output)?
                            }
                        }
                        None => {
                            parser.parse(input_field.output_name(), Some(default_value), output)?
                        }
                    }
                } else {
                    match input {
                        Some(val) => {
                            if val.is_empty() {
                                match parser {
                                    //only
                                    Parser::String()
                                    | Parser::StringToUpper()
                                    | Parser::StringToLower() => {
                                        parser.parse(input_field.output_name(), input, output)?;
                                    }
                                    Parser::Ignore() => {
                                        //don't do anything for Ignore parser
                                    }
                                    _ => output.add(input_field.output_name(), Value::Null()),
                                }
                            } else {
                                //input is not empty
                                parser.parse(input_field.output_name(), input, output)?
                            }
                        }
                        None => {
                            if let Parser::Ignore() = parser {
                                //don't do anything for Ignore parser
                            } else {
                                output.add(input_field.output_name(), Value::Null())
                            }
                        }
                    }
                }
            }
            Field::Multi(multi) => multi.parse(name, input, output)?,
            Field::Object {
                name: _,
                fields: _,
                ignore: _,
            } => {
                //does nothing, objects must be manually parsed
            }
            Field::Array(field) => field.0.parse(name, input, output)?,
        }
        Ok(())
    }

    /// Attempts to extract a value from the input string based on the field's configuration.
    ///
    /// This method is only applicable to `Field::Simple` variants. For `Field::Multi` and `Field::Object`,
    /// it returns `None` as these field types do not directly produce a single value.
    ///
    /// # Arguments
    /// * `input` - The input string to parse.
    pub fn get_value(&self, input: Option<&str>) -> Result<Option<Value>, Error> {
        let value = match self {
            Field::Single {
                name: _,
                parser,
                default_value,
            } => {
                if let Some(default_value) = default_value {
                    match input {
                        Some(val) => {
                            if val.is_empty() {
                                parser.get_value(Some(default_value))?
                            } else {
                                parser.get_value(input)?
                            }
                        }
                        None => parser.get_value(Some(default_value))?,
                    }
                } else {
                    match input {
                        Some(val) => {
                            if val.is_empty() {
                                match parser {
                                    // empty fields are only valid for String parsers
                                    Parser::String()
                                    | Parser::StringToUpper()
                                    | Parser::StringToLower() => parser.get_value(input)?,
                                    Parser::Ignore() => None,
                                    _ => Some(Value::Null()),
                                }
                            } else {
                                //input is not empty
                                parser.get_value(input)?
                            }
                        }
                        None => {
                            if let Parser::Ignore() = parser {
                                None
                            } else {
                                Some(Value::Null())
                            }
                        }
                    }
                }
            }
            Field::Multi(_) => None,
            Field::Object {
                name: _,
                fields: _,
                ignore: _,
            } => None,
            Field::Array(field) => field.0.get_value(input)?,
        };
        Ok(value)
    }

    /// Sets a specific value into the output map according to the field's configuration.
    ///
    /// This method is used to directly assign a pre-processed `Value` to the output map based on the field's
    /// parser configuration. It validates the value type against the field's expected type and inserts
    /// it into the output map under the field's configured output name.
    ///
    /// # Arguments
    ///
    /// * `value` - The `Value` to be inserted into the output map.
    /// * `output` - A mutable reference to the output map where the value will be stored.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the value was successfully inserted, or an `Error` if:
    /// - The value type doesn't match the field's expected type (e.g., non-integer for an integer field)
    /// - The field is a multi-input field (not supported for direct value setting)
    /// - The field is an object field but the provided value isn't an object
    ///
    pub fn set_value(&self, value: Value, output: &mut Record) -> Result<(), Error> {
        match self {
            Field::Single {
                name: input_field,
                parser: _,
                default_value: _,
            } => {
                output.insert(input_field.output_name().to_owned(), value);
            }
            Field::Multi(_) => {
                return Err(Error::ValueNotSupportedForMultiInputField(
                    self.output_name().to_owned(),
                ));
            }

            Field::Object {
                name,
                fields: _,
                ignore: _,
            } => {
                if let Value::Object(_) = value {
                    output.insert(name.output_name().to_owned(), value);
                } else {
                    return Err(Error::ValueNotObject(name.output_name().to_owned()));
                }
            }
            Field::Array(field) => field.0.set_value(value, output)?,
        }
        Ok(())
    }

    /// Returns a list of field names that this field will output.
    ///
    /// For `Simple` fields, combines the field's own name with any additional names produced by the parser.
    /// For `Multi` fields, returns the output field of the multi-parser.
    /// For `Object` fields, returns the field's own name.
    ///
    pub fn output_fields_names(&self) -> Vec<FieldName> {
        match self {
            Field::Single {
                name: input_field,
                parser,
                default_value: _,
            } => {
                let mut fields = parser.output_fields_names();
                if fields.is_empty() {
                    fields.push(input_field.clone());
                }
                fields
            }
            Field::Multi(multi) => vec![multi.output_field.clone()],
            Field::Object {
                name: input_field,
                fields: _,
                ignore: _,
            } => vec![input_field.clone()],
            Field::Array(field) => field.0.output_fields_names(),
        }
    }
}

#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
/// Combines several input fields into a single output using a `MultiParser`.
///
/// * `input_fields` – the source fields whose values are collected.
/// * `parser` – strategy (e.g., joining strings) that merges the values.
/// * `output_field` – metadata for the resulting combined field.
/// * `input_values` – thread‑safe cache storing the intermediate string values
///   for each input field.
pub struct MultiInputField {
    pub input_fields: Vec<Field>,
    pub parser: MultiParser,
    pub output_field: FieldName,
    pub input_values: Arc<Mutex<HashMap<String, Option<String>>>>,
}

#[pymethods]
impl MultiInputField {
    /// Creates a new `MultiInputField` instance.
    ///
    /// # Arguments
    ///
    /// * `input_fields` - A list of `Field` objects that define the input sources.
    /// * `output_field` - The metadata for the resulting output field.
    /// * `parser` - The `MultiParser` strategy to use for combining input values.
    ///
    /// # Returns
    ///
    /// A new `MultiInputField` with the provided configuration and an empty input value cache.
    ///
    #[new]
    pub fn new(input_fields: Vec<Field>, output_field: FieldName, parser: MultiParser) -> Self {
        MultiInputField {
            input_fields,
            parser,
            output_field,
            input_values: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
impl MultiInputField {
    /// Parses an input value for a specific field and accumulates it into a combined output.
    ///
    /// This method stores the input value under its corresponding field name in an internal cache
    /// (`input_values`). Once all expected input fields have been received (i.e., the number of
    /// fields matches the number of stored values), it triggers the `MultiParser` to combine
    /// the values into a single output field.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the input field being parsed (used as a key in `input_values`).
    /// * `input` - The raw string value to be stored and eventually combined.
    /// * `output` - A mutable reference to the output map where the final combined value will be inserted.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the input was successfully stored and, when complete, the combined value was written.
    /// Returns an `Error` if the `MultiParser` fails during combination (though this is currently not expected).
    ///
    /// # Behavior
    ///
    /// - Input values are stored in a thread-safe `HashMap` protected by a `Mutex`.
    /// - The parser only runs when all `input_fields` have been received.
    /// - After parsing, the input cache is cleared to prepare for the next round.
    ///
    pub fn parse(&self, name: &str, input: Option<&str>, output: &mut Record) -> Result<(), Error> {
        let mut input_values = self.input_values.lock().expect("mutex lock panicked");

        input_values.insert(name.to_string(), input.map(|s| s.to_string()));

        if self.input_fields.len() == input_values.len() {
            let mut ordered_dict = IndexMap::new();
            for field in &self.input_fields {
                if let Field::Single {
                    name: input_field,
                    parser: _,
                    default_value: _,
                } = field
                {
                    let key = input_field.input_name();
                    if let Some(val) = input_values.get(key) {
                        ordered_dict.insert(key.to_owned(), val.clone());
                    }
                }
            }
            self.parser
                .parse(&ordered_dict, self.output_field.output_name(), output)?;
            input_values.clear();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {

    use crate::DateOutputCodec;

    use super::*;

    #[test]
    fn to_json_array() {
        let mut record = Record::new();
        record.add(
            "Array",
            Value::Array(vec![
                Value::String("test \"\n\r\"erse".to_owned()),
                Value::String("test2".to_owned()),
            ]),
        );

        let tup = Value::Object(record);
        let mut buffer = String::new();
        Value::json_serialise_value(&tup, &mut buffer, &DateOutputCodec::Iso(), true).unwrap();
        let expected = r#"{"Array":["test \"\n\"erse","test2"]}"#;
        assert_eq!(expected, buffer);
    }

    #[test]
    fn parsers() {
        //Bool
        let mut output = Record::new();
        let parser = Parser::Bool();
        parser.parse("value", Some("no"), &mut output).unwrap();

        if let &Value::Bool(value) = output.get("value").unwrap() {
            assert_eq!(value, false);
        } else {
            panic!("should be a boolean");
        }

        //Float
        let mut output = Record::new();
        let parser = Parser::Float();
        parser.parse("value", Some("3.5"), &mut output).unwrap();

        if let &Value::Float(value) = output.get("value").unwrap() {
            assert_eq!(value, 3.5);
        } else {
            panic!("should be a Float");
        }

        //Ignore
        let mut output = Record::new();
        let parser = Parser::Ignore();
        parser.parse("value", Some("3.5"), &mut output).unwrap();

        if let Some(_) = output.get("value") {
            panic!("should be ignored");
        }

        //Integer
        let mut output = Record::new();
        let parser = Parser::Int();
        parser.parse("value", Some("42"), &mut output).unwrap();

        if let &Value::Int(value) = output.get("value").unwrap() {
            assert_eq!(value, 42);
        } else {
            panic!("should be an Integer");
        }

        //IntRadix
        let mut output = Record::new();
        let parser = Parser::IntRadix(16);
        parser
            .parse("value", Some("0x0000000004571C88"), &mut output)
            .unwrap();

        if let &Value::Int(value) = output.get("value").unwrap() {
            assert_eq!(value, 72817800);
        } else {
            panic!("should be an Integer");
        }

        //IntToHex
        let number = Some("4096");
        let parser = Parser::IntToHex(8);
        let val = parser.get_value(number).unwrap().unwrap();
        if let Value::String(value) = val {
            assert_eq!("0x00001000", value)
        } else {
            panic!("should be an String");
        }

        //IntToHex not zero-padded
        let number = Some("4096");
        let parser = Parser::IntToHex(0);
        let val = parser.get_value(number).unwrap().unwrap();
        if let Value::String(value) = val {
            assert_eq!("0x1000", value)
        } else {
            panic!("should be an String");
        }

        //String
        let mut output = Record::new();
        let parser = Parser::String();
        parser.parse("value", Some("test"), &mut output).unwrap();

        if let &Value::String(value) = &output.get("value").unwrap() {
            assert_eq!(value, "test");
        } else {
            panic!("should be an String");
        }

        //StringToUpper
        let mut output = Record::new();
        let parser = Parser::StringToUpper();
        parser.parse("value", Some("test"), &mut output).unwrap();

        if let &Value::String(value) = &output.get("value").unwrap() {
            assert_eq!(value, "TEST");
        } else {
            panic!("should be an String");
        }

        //Split
        let mut output = Record::new();
        let parser = Parser::Split(",".to_owned());
        parser
            .parse("value", Some("test,test2"), &mut output)
            .unwrap();

        if let &Value::Array(_) = &output.get("value").unwrap() {
        } else {
            panic!("should be an Array");
        }

        //date
        let mut output = Record::new();
        let parser = Parser::DateTime(DateInputCodec::Iso());
        parser
            .parse("value", Some("1996-12-19T16:39:57.123-08:00"), &mut output)
            .unwrap();

        if let &Value::Date(_) = &output.get("value").unwrap() {
        } else {
            panic!("should be a Date");
        }
    }

    #[test]
    fn parsers_dynamic() {
        //Bool
        let mut output = Record::new();

        let parser = Parser::Dynamic(vec![
            Parser::DateTime(DateInputCodec::Iso()),
            Parser::Int(),
            Parser::Float(),
            Parser::String(),
        ]);

        parser.parse("value", Some("12"), &mut output).unwrap();
        if let &Value::Int(value) = output.get("value").unwrap() {
            assert_eq!(value, 12);
        } else {
            panic!("should be an Integer");
        }

        parser.parse("value", Some("3.14"), &mut output).unwrap();
        if let &Value::Float(value) = output.get("value").unwrap() {
            assert_eq!(value, 3.14);
        } else {
            panic!("should be a float");
        }

        parser
            .parse("value", Some("1996-12-19T16:39:57.123-08:00"), &mut output)
            .unwrap();
        if let &Value::Date(_) = output.get("value").unwrap() {
        } else {
            panic!("should be a date");
        }

        let bad_date = Some("1996/12-19T16:39:57.123-08:00");
        parser.parse("value", bad_date, &mut output).unwrap();
        if let &Value::String(data) = output.get("value").as_ref().unwrap() {
            assert_eq!(Some(data.as_str()), bad_date);
        } else {
            panic!("should be a string");
        }
    }

    #[test]
    fn field_set_value_multi() {
        let field = Field::Multi(MultiInputField::new(
            vec![Field::Single {
                name: FieldName::new("input_name".to_owned(), false, None, None, None, None),
                parser: Parser::Bool(),
                default_value: None,
            }],
            FieldName::new("input_name".to_owned(), false, None, None, None, None),
            MultiParser::Join("()".to_owned(), true),
        ));
        let mut record = Record::new();
        if let Ok(_) = field.set_value(Value::Int(0), &mut record) {
            panic!("set value is not supported for multi imput field")
        }
    }

    #[test]
    fn field_set_value_object() {
        let field = Field::Object {
            name: FieldName::new("input_name".to_owned(), false, None, None, None, None),
            fields: vec![],
            ignore: false,
        };

        let mut record = Record::new();
        if let Ok(_) = field.set_value(Value::Int(0), &mut record) {
            panic!("set value requires an Object Value for Object field")
        }

        let object_record = Record::new();
        field
            .set_value(Value::Object(object_record), &mut record)
            .unwrap()
    }

    #[test]
    fn field_set_value_simple() {
        //booleans
        let field = Field::Single {
            name: FieldName::new("input_name".to_owned(), false, None, None, None, None),
            parser: Parser::Bool(),
            default_value: None,
        };
        let mut record = Record::new();
        field.set_value(Value::Null(), &mut record).unwrap();
        field.set_value(Value::Bool(true), &mut record).unwrap();
    }

    #[test]
    fn field_parse_default() {
        //booleans
        let field = Field::Single {
            name: FieldName::new("input_name".to_owned(), false, None, None, None, None),
            parser: Parser::Bool(),
            default_value: Some("false".to_string()),
        };

        let val = field.get_value(Some("")).unwrap().unwrap();
        if let Value::Bool(s) = val {
            assert!(!s)
        } else {
            panic!("should be a boolean")
        }

        let val = field.get_value(None).unwrap().unwrap();
        if let Value::Bool(s) = val {
            assert!(!s)
        } else {
            panic!("should be a boolean")
        }
    }
}
