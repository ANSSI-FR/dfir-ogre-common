use crate::{
    FieldName, Parser, Record, Value,
    errors::Error,
    field::{ArrayField, Field},
};

use indexmap::IndexMap;
use log::warn;
use pyo3::prelude::*;

/// A struct that manages the parsing of an input fields into a `Record`.
///
/// `FieldParsers` holds a name for the input source and a list of `Field` parsers,
/// which are applied in sequence to parse the input string and populate the output record.
/// Each `Field` parser is responsible for extracting and validating the field value
///

#[derive(Debug, Clone)]
#[pyclass(from_py_object)]
pub struct FieldParser {
    /// The name of the input field being parsed.
    pub input_name: String,

    pub fields: Vec<Field>,
}

impl FieldParser {
    /// Creates a new `FieldParsers` instance with the given input name and list of field parsers.
    ///
    /// # Arguments
    ///
    /// * `input_name` - A string representing the name of the input field.
    /// * `fields` - A vector of `Field` parsers that will be applied in order during parsing.
    ///
    /// # Returns
    ///
    /// A new `FieldParsers` instance initialized with the provided input name and field parsers.
    pub fn new(input_name: String, fields: Vec<Field>) -> Self {
        FieldParser {
            input_name: input_name.to_string(),
            fields,
        }
    }
}

#[pymethods]
impl FieldParser {
    /// Parses the input string and populates the output record using the registered field parsers.
    ///
    /// This method applies each `Field` parser in sequence, passing the input string and populating the
    /// provided `Record`. If any parser fails, the error is propagated immediately.
    ///
    /// # Arguments
    ///
    /// * `input` - The input string to be parsed.
    /// * `output` - A mutable reference to a `Record` where parsed field values will be stored.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all fields were successfully parsed, or an `Error` if any parser fails.
    ///
    /// # Errors
    ///
    /// This method may return an error if any field parser encounters invalid input,
    /// missing data, or a parsing issue.
    pub fn parse(&self, input: Option<&str>, output: &mut Record) -> Result<(), Error> {
        for field in &self.fields {
            field.parse(&self.input_name, input, output)?;
        }
        Ok(())
    }

    /// Parses the input string and returns the first non-None value from the registered fields.
    ///
    /// # Arguments
    /// * `input` - The input string to be parsed.
    pub fn parse_into_value(&self, input: Option<&str>) -> Result<Option<Value>, Error> {
        let mut return_value = None;
        for field in &self.fields {
            let value = field.get_value(input)?;
            if value.is_some() {
                return_value = value;
                break;
            }
        }
        Ok(return_value)
    }

    /// Sets the provided value into the output with the correct name.
    ///
    /// # Arguments
    /// * `value` - The Value to be set in the output record.
    /// * `output` - A mutable reference to the `Record` where values will be stored.
    ///
    pub fn set_value(&self, value: Value, output: &mut Record) -> Result<(), Error> {
        if self.fields.len() == 1 {
            self.fields[0].set_value(value, output)?;
        } else {
            for field in &self.fields {
                field.set_value(value.clone(), output)?;
            }
        }
        Ok(())
    }

    /// Returns the name of the input source associated with this parser.
    pub fn input_name(&self) -> String {
        self.input_name.clone()
    }
}

/// Represents the various kinds of parsers that can be associated with a field.
///
/// * `Field` – a straightforward parser that works on a single input field.
/// * `Array` – wraps a parser for handling arrays of values, stored in a `BoxedParserType`.
/// * `Object` – contains a nested `FieldParserTree` for parsing complex objects.

#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub enum ParserType {
    /// Direct parser for a single input field.
    Field { parser: FieldParser },

    /// Parser for an array of values, encapsulated in a `BoxedParserType`.
    Array { field: BoxedParserType },

    /// Parser for nested object structures, using a `FieldParserTree`.
    Object { field_parsers: FieldParserTree },
}

/// This wrapper is needed because Rust enums cannot store a recursive type directly.
/// By boxing the inner `ParserType`, we avoid infinite size recursion while still
/// providing a convenient API for the rest of the code.
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub struct BoxedParserType(pub Box<ParserType>);

/// Organizes field parsers from a `Mapping`` hashmap into a hierarchical structure, supporting both
/// direct field lookups and nested path-based access.
///
/// It also handles fallback parsing using a default parser when specific field mappings are missing.
///
/// # Fields
/// * `parsers` - A map from field names to their corresponding parser types (either direct parsers or nested objects)
/// * `default_parser` - An optional parser used as a fallback for fields without explicit mappings
#[derive(Clone, Debug, Default)]
#[pyclass(from_py_object)]
pub struct FieldParserTree {
    pub field_name: Option<FieldName>,
    pub parsers: IndexMap<String, ParserType>,
    pub output_fields: Vec<Field>,
    pub default_parser: Option<Parser>,
    pub ignore_parsing: bool,
    pub has_primary_keys: bool,
}
impl FieldParserTree {
    /// Creates a new `FieldParserTree` from a list of `Field` definitions.
    ///
    /// * `field_name` – optional name for the current node (used for nested objects).
    /// * `mapping` – slice of `Field` objects describing how to parse each input field.
    /// * `default_parser` – optional fallback parser used when a field lacks an explicit mapping.
    /// * `ignore_parsing` – when `true`, parsing for this subtree is skipped.
    ///
    /// The function builds an internal `IndexMap` of parsers by delegating to
    /// `parse_mapping`, which walks the field definitions recursively.
    pub fn new(
        field_name: Option<FieldName>,
        mapping: &Vec<Field>,
        default_parser: Option<Parser>,
        ignore_parsing: bool,
    ) -> Self {
        let mut parsers: IndexMap<String, ParserType> = IndexMap::with_capacity(mapping.len());
        let mut output_fields = Vec::with_capacity(mapping.len());
        let has_primary_keys =
            Self::parse_mapping(mapping, &mut parsers, &mut output_fields, &default_parser);
        Self {
            field_name,
            parsers,
            output_fields,
            default_parser,
            ignore_parsing,
            has_primary_keys,
        }
    }

    /// Recursively walks the list of `Field` definitions and populates the internal parser map.
    ///
    /// This helper builds the hierarchical structure that powers the `FieldParserTree`.
    ///
    /// # Arguments
    /// * `mapping` – a slice of `Field` objects describing how each input should be parsed.
    /// * `parsers` – the mutable `IndexMap` that will hold the generated `ParserType` entries.
    /// * `default_parser` – the optional fallback parser used when a field lacks an explicit mapping.
    pub fn parse_mapping(
        mapping: &Vec<Field>,
        parsers: &mut IndexMap<String, ParserType>,
        output_fields: &mut Vec<Field>,
        default_parser: &Option<Parser>,
    ) -> bool {
        let mut has_primary_keys = false;

        for field in mapping {
            match field {
                Field::Single {
                    name,
                    parser,
                    default_value: _,
                } => {
                    if name.primary_key {
                        has_primary_keys = true;
                    }

                    // create output_field
                    match parser {
                        Parser::Python(_) | Parser::Extension(_) => {
                            for name in parser.output_fields_names() {
                                let field = Field::Single {
                                    name,
                                    parser: Parser::String(),
                                    default_value: None,
                                };
                                output_fields.push(field);
                            }
                        }
                        _ => {
                            output_fields.push(field.clone());
                        }
                    }

                    // create parser
                    let parser_type = parsers.get_mut(name.input_name());
                    if let Some(parser_type) = parser_type {
                        if let ParserType::Field { parser } = parser_type {
                            parser.fields.push(field.clone());
                        }
                    } else {
                        let field_parser =
                            FieldParser::new(name.input_name().to_string(), vec![field.clone()]);
                        let fp_type = ParserType::Field {
                            parser: field_parser,
                        };
                        parsers.insert(name.input_name().to_string(), fp_type);
                    }
                }
                Field::Multi(_) => {
                    output_fields.push(field.clone());
                    for name in field.input_names() {
                        let parser_type = parsers.get_mut(&name);
                        if let Some(parser_type) = parser_type {
                            if let ParserType::Field { parser } = parser_type {
                                parser.fields.push(field.clone());
                            }
                        } else {
                            let field_parser = FieldParser::new(name.clone(), vec![field.clone()]);
                            let fp_type = ParserType::Field {
                                parser: field_parser,
                            };
                            parsers.insert(name, fp_type);
                        }
                    }
                }
                Field::Object {
                    name,
                    fields,
                    ignore,
                } => {
                    let mut inner_parser: IndexMap<String, ParserType> = IndexMap::new();

                    let mut inner_fields = Vec::with_capacity(fields.len());
                    let inner_has_primary = Self::parse_mapping(
                        fields,
                        &mut inner_parser,
                        &mut inner_fields,
                        default_parser,
                    );
                    //create output fields
                    output_fields.push(Field::Object {
                        name: name.clone(),
                        fields: inner_fields.clone(),
                        ignore: *ignore,
                    });

                    if inner_has_primary {
                        has_primary_keys = true;
                    }
                    let default_parser = if *ignore {
                        Some(Parser::Ignore())
                    } else {
                        default_parser.clone()
                    };

                    parsers.insert(
                        name.input_name().to_string(),
                        ParserType::Object {
                            field_parsers: FieldParserTree {
                                field_name: Some(name.clone()),
                                parsers: inner_parser,
                                output_fields: inner_fields,
                                default_parser,
                                ignore_parsing: *ignore,
                                has_primary_keys: false,
                            },
                        },
                    );
                }

                Field::Array(array_field) => {
                    let array_field = array_field.0.as_ref();
                    for name in array_field.input_names() {
                        match array_field {
                            Field::Single {
                                name,
                                parser,
                                default_value: _,
                            } => {
                                // create output_field
                                match parser {
                                    Parser::Python(_) | Parser::Extension(_) => {
                                        for name in parser.output_fields_names() {
                                            let inner_field = Field::Single {
                                                name,
                                                parser: Parser::String(),
                                                default_value: None,
                                            };

                                            output_fields.push(Field::Array(ArrayField(Box::new(
                                                inner_field,
                                            ))));
                                        }
                                    }
                                    _ => {
                                        output_fields.push(field.clone());
                                    }
                                }

                                let field_parser = FieldParser::new(
                                    name.input_name().to_string(),
                                    vec![field.clone()],
                                );
                                let fp_type = Box::new(ParserType::Field {
                                    parser: field_parser,
                                });
                                parsers.insert(
                                    name.input_name().to_string(),
                                    ParserType::Array {
                                        field: BoxedParserType(fp_type),
                                    },
                                );
                            }
                            Field::Multi(_) => {
                                let field_parser =
                                    FieldParser::new(name.clone(), vec![field.clone()]);

                                let fp_type = Box::new(ParserType::Field {
                                    parser: field_parser,
                                });

                                parsers.insert(
                                    name,
                                    ParserType::Array {
                                        field: BoxedParserType(fp_type),
                                    },
                                );
                                output_fields.push(field.clone());
                            }
                            Field::Array(_) => {
                                warn!(
                                    "for field mapping '{:#?}' array of arrays is not supported, the field is ignored",
                                    field.name()
                                );
                            }
                            Field::Object {
                                name,
                                fields,
                                ignore,
                            } => {
                                let mut inner_parser = IndexMap::with_capacity(fields.len());
                                let mut inner_fields = Vec::with_capacity(fields.len());

                                let inner_has_primary = Self::parse_mapping(
                                    fields,
                                    &mut inner_parser,
                                    &mut inner_fields,
                                    default_parser,
                                );

                                //create output fields
                                let output_inner_field = Field::Object {
                                    name: name.clone(),
                                    fields: inner_fields.clone(),
                                    ignore: *ignore,
                                };

                                output_fields
                                    .push(Field::Array(ArrayField(Box::new(output_inner_field))));

                                //create parser
                                if inner_has_primary {
                                    has_primary_keys = true;
                                }
                                let default_parser = if *ignore {
                                    Some(Parser::Ignore())
                                } else {
                                    default_parser.clone()
                                };
                                let parser_type = Box::new(ParserType::Object {
                                    field_parsers: FieldParserTree {
                                        field_name: Some(name.clone()),
                                        parsers: inner_parser,
                                        output_fields: inner_fields,
                                        default_parser,
                                        ignore_parsing: *ignore,
                                        has_primary_keys: false,
                                    },
                                });

                                parsers.insert(
                                    name.input_name().to_string(),
                                    ParserType::Array {
                                        field: BoxedParserType(parser_type),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        has_primary_keys
    }
}
#[pymethods]
impl FieldParserTree {
    /// Retrieves a parser for a specific field name.
    ///
    /// This method first checks for an exact match of the input name. If not found,
    /// it uses the default parser if available. It does not support nested object paths.
    ///
    /// # Arguments
    /// * `input_name` - The name of the field to retrieve a parser for
    pub fn get_parser(&mut self, input_name: &str) -> Option<FieldParser> {
        let parser = self.parsers.get(input_name);

        match parser {
            Some(fp_type) => match fp_type {
                ParserType::Field { parser } => Some(parser.clone()),
                ParserType::Array { field } => {
                    let parser_type = field.0.as_ref();
                    match parser_type {
                        ParserType::Field { parser } => Some(parser.clone()),
                        ParserType::Array { field: _ } => None,
                        ParserType::Object { field_parsers: _ } => None,
                    }
                }
                ParserType::Object { field_parsers: _ } => None,
            },
            None => {
                if let Some(defautl) = &self.default_parser {
                    let field_parser = FieldParser::new(
                        input_name.to_owned(),
                        vec![Field::Single {
                            name: FieldName::new(
                                input_name.to_owned(),
                                false,
                                None,
                                None,
                                None,
                            ),
                            parser: defautl.clone(),
                            default_value: None,
                        }],
                    );
                    self.parsers.insert(
                        input_name.to_owned(),
                        ParserType::Field {
                            parser: field_parser.clone(),
                        },
                    );
                    Some(field_parser)
                } else {
                    None
                }
            }
        }
    }

    /// Retrieves a parser for a nested field path.
    ///
    /// This method supports accessing nested fields through a vector of names.
    /// For example, `["System", "TimeCreated", "SystemTime"]` would navigate
    /// through nested objects to find a parser.
    ///
    /// # Arguments
    /// * `path` - A vector of field names representing the nested path
    ///
    pub fn get_parser_by_path(&mut self, path: Vec<String>) -> Option<FieldParser> {
        let mut parsers_map = &self.parsers;
        let path_max_pos = path.len() - 1;
        for (pos, input_name) in path.iter().enumerate() {
            let parser = parsers_map.get(input_name);

            match parser {
                Some(fp_type) => match fp_type {
                    ParserType::Field { parser } => {
                        if pos == path_max_pos {
                            return Some(parser.clone());
                        } else {
                            return None;
                        }
                    }
                    ParserType::Array { field } => {
                        if pos == path_max_pos {
                            let parser_type = field.0.as_ref();
                            let parser = match parser_type {
                                ParserType::Field { parser } => Some(parser.clone()),
                                ParserType::Array { field: _ } => None,
                                ParserType::Object { field_parsers: _ } => None,
                            };

                            return parser;
                        } else {
                            return None;
                        }
                    }
                    ParserType::Object { field_parsers } => {
                        if pos == path_max_pos {
                            return None;
                        }
                        parsers_map = &field_parsers.parsers;
                    }
                },
                None => {
                    if let Some(defautl) = &self.default_parser {
                        let field_parser = FieldParser::new(
                            input_name.to_owned(),
                            vec![Field::Single {
                                name: FieldName::new(
                                    input_name.to_owned(),
                                    false,
                                    None,
                                    None,
                                    None,
                                ),
                                parser: defautl.clone(),
                                default_value: None,
                            }],
                        );
                        self.parsers.insert(
                            input_name.to_owned(),
                            ParserType::Field {
                                parser: field_parser.clone(),
                            },
                        );
                        return Some(field_parser);
                    } else {
                        return None;
                    }
                }
            }
        }
        None
    }

    /// Retrieves a nested `FieldParserTree` for a specific field name, if it exists.
    ///
    /// This method allows access to the parser hierarchy for nested object fields. This is useful for working with hierarchical data structures where
    /// fields contain sub-fields that need to be parsed recursively.
    ///
    /// # Arguments
    ///
    /// * `input_name` - The name of the field to look up in the parser registry.
    ///
    pub fn get_parser_subtree(&self, input_name: &str) -> Option<FieldParserTree> {
        let parser = self.parsers.get(input_name);

        match parser {
            Some(fp_type) => match fp_type {
                ParserType::Field { parser: _ } | ParserType::Array { field: _ } => None,
                ParserType::Object { field_parsers } => Some(field_parsers.clone()),
            },
            None => None,
        }
    }

    /// Parses a single input field using the appropriate parser.
    ///
    /// * `input_name` – the name of the field as it appears in the source data.
    /// * `value` – the raw string value extracted from the input, if any.
    /// * `record` – the mutable record that will receive the parsed output.
    ///
    /// The method first looks up a dedicated `ParserType::Field`. If found, it
    /// delegates parsing to that `FieldParser`. Otherwise it falls back to the
    /// tree's `default_parser` if one is configured. When no parser matches,
    /// the call succeeds silently, leaving the output unchanged.
    pub fn parse(
        &self,
        input_name: &str,
        value: Option<&str>,
        record: &mut Record,
    ) -> Result<(), Error> {
        let parser = self.parsers.get(input_name);
        match parser {
            Some(parser_type) => {
                if let ParserType::Field { parser } = parser_type {
                    parser.parse(value, record)
                } else {
                    Err(Error::InvalidParserType(input_name.to_string()))
                }
            }
            None => {
                if let Some(default) = &self.default_parser {
                    default.parse(input_name, value, record)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Sets a parsed value into the output record, handling different parser shapes.
    ///
    /// * `input_name` – the source field name associated with the value.
    /// * `value` – the parsed `Value` ready to be stored.
    /// * `record` – the mutable record that will receive the value.
    ///
    /// The method resolves the appropriate parser for `input_name`. For a plain
    /// `ParserType::Field`, it forwards the value to that field's `set_value`.
    /// For arrays, it extracts the inner `Field` parser and stores the value,
    /// unless the array nests another array (which is unsupported). When the
    /// parser represents an object, the method adds the value under the object's
    /// output name. If no dedicated parser exists, the value is stored using the
    /// raw `input_name` as the key.
    pub fn set_field_value(
        &self,
        input_name: &str,
        value: Value,
        record: &mut Record,
    ) -> Result<(), Error> {
        let parser = self.parsers.get(input_name);
        match parser {
            Some(parser_type) => match parser_type {
                ParserType::Field { parser } => parser.set_value(value, record),
                ParserType::Array { field } => match field.0.as_ref() {
                    ParserType::Field { parser } => parser.set_value(value, record),

                    ParserType::Object { field_parsers } => {
                        if let Some(field) = &field_parsers.field_name {
                            record.add(field.output_name(), value)
                        }
                        Ok(())
                    }
                    ParserType::Array { field: _ } => {
                        Err(Error::UnsupportedNestingArray(input_name.to_string()))
                    }
                },
                ParserType::Object { field_parsers } => {
                    if let Some(field) = &field_parsers.field_name {
                        record.add(field.output_name(), value)
                    }
                    Ok(())
                }
            },
            None => {
                record.add(input_name, value);
                Ok(())
            }
        }
    }

    ///
    /// Retrieve the output name for this 'FieldParserTree'
    ///
    pub fn get_output_name(&self) -> String {
        match &self.field_name {
            Some(field) => field.output_name().to_string(),
            None => "".to_string(),
        }
    }
}

/// Manages field mappings for data parsing, organizing fields into a structured hierarchy.
/// This struct facilitates the conversion of input data into a `Record` by maintaining
/// a mapping of field names to their parsing configurations.
///
/// # Fields
/// * `mapping` - A hierarchical index map representing the structure of fields and their parsers.
/// * `field_parser_tree` - A collection of field parsers organized in a tree for efficient lookup and processing.
#[derive(Clone, Debug)]
#[pyclass(from_py_object)]
pub struct FieldMapping {
    pub field_parser_tree: FieldParserTree,
    pub last_field: Option<Field>,
}

#[pymethods]
impl FieldMapping {
    /// Creates a new `FieldMapping` instance from a list of `Field` definitions.
    ///
    /// This constructor builds a hierarchical mapping structure from the provided fields,
    /// automatically organizing them into nested objects where applicable. It also
    /// initializes a parser hierarchy for efficient field lookup.
    ///
    /// # Arguments
    ///
    /// * `mapping` - A vector of `Field` definitions that describe the structure of the data.
    /// * `default_parser` - An optional parser to use as a fallback for fields without explicit mappings.
    ///
    #[new]
    #[pyo3(signature = (mapping, default_parser=None))]
    pub fn new(mapping: Vec<Field>, default_parser: Option<Parser>) -> Self {
        let field_parser_tree = FieldParserTree::new(None, &mapping, default_parser.clone(), false);
        let last_field = mapping.last().cloned();
        FieldMapping {
            // input_mapping: mapping_dict,
            field_parser_tree,
            last_field,
        }
    }

    /// Returns a clone of the internal `FieldParserTree` instance.
    ///
    /// This method provides access to the parser hierarchy built from the field mappings,
    /// allowing for programmatic lookup of parsers by field name or path.
    pub fn get_field_parser_tree(&self) -> FieldParserTree {
        self.field_parser_tree.clone()
    }

    /// Retrieves a parser for a specific field name.
    ///
    /// This method first checks for an exact match of the input name. If not found,
    /// it uses the default parser if available. It does not support nested object paths.
    ///
    /// # Arguments
    /// * `input_name` - The name of the field to retrieve a parser for
    pub fn get_parser(&mut self, input_name: &str) -> Option<FieldParser> {
        self.field_parser_tree.get_parser(input_name)
    }

    /// Retrieves a parser for a nested field path.
    ///
    /// This method supports accessing nested fields through a vector of names.
    /// For example, `["System", "TimeCreated", "SystemTime"]` would navigate
    /// through nested objects to find a parser.
    ///
    /// # Arguments
    /// * `path` - A vector of field names representing the nested path
    ///
    pub fn get_parser_by_path(&mut self, path: Vec<String>) -> Option<FieldParser> {
        self.field_parser_tree.get_parser_by_path(path)
    }

    /// Retrieves a nested `FieldParserTree` for a specific field name, if it exists.
    ///
    /// This method allows access to the parser hierarchy for nested object fields. This is useful for working with hierarchical data structures where
    /// fields contain sub-fields that need to be parsed recursively.
    ///
    /// # Arguments
    ///
    /// * `input_name` - The name of the field to look up in the parser registry.
    ///
    pub fn get_parser_subtree(&self, input_name: &str) -> Option<FieldParserTree> {
        self.field_parser_tree.get_parser_subtree(input_name)
    }
}

#[cfg(test)]
mod tests {

    use crate::{DateInputCodec, field::ArrayField};

    use super::*;

    #[test]
    fn field_parsers_single_field() {
        let mapping = FieldMapping::new(field_mapping(), None);

        let mut field_parser = mapping.get_field_parser_tree();

        field_parser.get_parser("timestamp").unwrap();

        if let Some(_) = field_parser.get_parser("unknow_field") {
            panic!("shoudl not return a parser")
        }

        let mapping = FieldMapping::new(field_mapping(), Some(Parser::String()));
        let mut field_parser = mapping.get_field_parser_tree();
        field_parser.get_parser("unknow_field").unwrap();

        if let Some(_) = field_parser.get_parser("System") {
            panic!("System is an object an cannot return a parser")
        }
    }

    #[test]
    fn field_parsers_path() {
        let mapping = FieldMapping::new(field_mapping(), None);

        let mut field_parser = mapping.get_field_parser_tree();

        field_parser
            .get_parser_by_path(vec!["timestamp".to_owned()])
            .unwrap();

        if let Some(_) = field_parser.get_parser_by_path(vec!["unknow_field".to_owned()]) {
            panic!("shoudl not return a parser")
        }

        if let Some(_) = field_parser.get_parser_by_path(vec![
            "System".to_owned(),
            "TimeCreated_attributes".to_owned(),
            "unknow_field".to_owned(),
        ]) {
            panic!("shoudl not return a parser")
        }

        let mapping = FieldMapping::new(field_mapping(), Some(Parser::String()));
        let mut field_parser = mapping.get_field_parser_tree();

        field_parser
            .get_parser_by_path(vec!["unknow_field".to_owned()])
            .unwrap();

        if let Some(_) = field_parser.get_parser_by_path(vec!["System".to_owned()]) {
            panic!("System is an object an cannot return parser")
        }

        field_parser
            .get_parser_by_path(vec!["System".to_owned(), "EventRecordID".to_owned()])
            .unwrap();

        if let Some(_) = field_parser.get_parser_by_path(vec![
            "System".to_owned(),
            "TimeCreated_attributes".to_owned(),
        ]) {
            panic!("System->TimeCreated_attributes is an object an cannot return a parser")
        }

        field_parser
            .get_parser_by_path(vec![
                "System".to_owned(),
                "TimeCreated_attributes".to_owned(),
                "unknow_field".to_owned(),
            ])
            .unwrap();

        field_parser
            .get_parser_by_path(vec![
                "System".to_owned(),
                "TimeCreated_attributes".to_owned(),
                "SystemTime".to_owned(),
            ])
            .unwrap();

        field_parser
            .get_parser_by_path(vec![
                "EventData".to_owned(),
                "unknown".to_owned(),
                "unknown".to_owned(),
            ])
            .unwrap();
    }

    #[test]
    fn parse_uses_default_parser_for_unknown_fields() {
        let mapping = FieldMapping::new(vec![], Some(Parser::Int()));
        let tree = mapping.get_field_parser_tree();
        let mut record = Record::new();

        tree.parse("unknown_count", Some("42"), &mut record)
            .unwrap();

        assert_eq!(record.get("unknown_count"), Some(&Value::Int(42)));
    }

    #[test]
    fn parse_rejects_object_parser_type() {
        let mapping = FieldMapping::new(field_mapping(), None);
        let tree = mapping.get_field_parser_tree();
        let mut record = Record::new();

        let error = tree
            .parse("System", Some("ignored"), &mut record)
            .unwrap_err();

        assert!(matches!(
            error,
            crate::errors::Error::InvalidParserType(name) if name == "System"
        ));
    }

    #[test]
    fn parser_subtree_reports_output_name() {
        let mapping = FieldMapping::new(field_mapping(), None);
        let system = mapping.get_parser_subtree("System").unwrap();

        assert_eq!(system.get_output_name(), "system");
        assert!(
            system
                .get_parser_subtree("TimeCreated_attributes")
                .is_some()
        );
        assert!(system.get_parser_subtree("EventRecordID").is_none());
    }

    #[test]
    fn set_field_value_uses_output_names_for_objects_and_unknowns() {
        let mapping = FieldMapping::new(field_mapping(), None);
        let tree = mapping.get_field_parser_tree();
        let mut record = Record::new();

        tree.set_field_value("timestamp", Value::String("raw".to_owned()), &mut record)
            .unwrap();
        assert_eq!(
            record.get("timestamp"),
            Some(&Value::String("raw".to_owned()))
        );

        let mut system = Record::new();
        system.add("event_record_id", Value::Int(7));
        tree.set_field_value("System", Value::Object(system.clone()), &mut record)
            .unwrap();
        assert_eq!(record.get("system"), Some(&Value::Object(system)));

        tree.set_field_value("unknown", Value::Bool(true), &mut record)
            .unwrap();
        assert_eq!(record.get("unknown"), Some(&Value::Bool(true)));
    }

    #[test]
    fn set_field_value_handles_array_mapped_objects() {
        let mapping = FieldMapping::new(
            vec![Field::Array(ArrayField(Box::new(Field::Object {
                name: FieldName::new(
                    "Items".to_owned(),
                    false,
                    Some("items".to_owned()),
                    None,
                    None,
                ),
                ignore: false,
                fields: vec![Field::Single {
                    name: FieldName::new(
                        "Name".to_owned(),
                        false,
                        Some("name".to_owned()),
                        None,
                        None,
                    ),
                    parser: Parser::String(),
                    default_value: None,
                }],
            })))],
            None,
        );
        let tree = mapping.get_field_parser_tree();
        let mut record = Record::new();
        let mut item = Record::new();
        item.add("name", Value::String("first".to_owned()));

        tree.set_field_value("Items", Value::Object(item.clone()), &mut record)
            .unwrap();

        assert_eq!(record.get("items"), Some(&Value::Object(item)));
    }

    #[test]
    fn field_parser_parse_into_value_skips_ignored_fields() {
        let parser = FieldParser::new(
            "count".to_owned(),
            vec![
                Field::Single {
                    name: FieldName::new("ignored".to_owned(), false, None, None, None),
                    parser: Parser::Ignore(),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("count".to_owned(), false, None, None, None),
                    parser: Parser::Int(),
                    default_value: None,
                },
            ],
        );

        let value = parser.parse_into_value(Some("42")).unwrap();

        assert_eq!(value, Some(Value::Int(42)));
    }

    #[test]
    fn field_parser_set_value_updates_all_shared_input_fields() {
        let parser = FieldParser::new(
            "shared".to_owned(),
            vec![
                Field::Single {
                    name: FieldName::new(
                        "shared".to_owned(),
                        false,
                        Some("first".to_owned()),
                        None,
                        None,
                    ),
                    parser: Parser::String(),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new(
                        "shared".to_owned(),
                        false,
                        Some("second".to_owned()),
                        None,
                        None,
                    ),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
        );
        let mut record = Record::new();

        parser
            .set_value(Value::String("same".to_owned()), &mut record)
            .unwrap();

        assert_eq!(record.get("first"), Some(&Value::String("same".to_owned())));
        assert_eq!(
            record.get("second"),
            Some(&Value::String("same".to_owned()))
        );
    }

    #[test]
    fn set_field_value_rejects_nested_array_parser() {
        let leaf_parser = FieldParser::new(
            "items".to_owned(),
            vec![Field::Single {
                name: FieldName::new("items".to_owned(), false, None, None, None),
                parser: Parser::String(),
                default_value: None,
            }],
        );
        let nested_array = ParserType::Array {
            field: BoxedParserType(Box::new(ParserType::Field {
                parser: leaf_parser,
            })),
        };
        let tree = FieldParserTree {
            parsers: indexmap::IndexMap::from([(
                "items".to_owned(),
                ParserType::Array {
                    field: BoxedParserType(Box::new(nested_array)),
                },
            )]),
            ..Default::default()
        };
        let mut record = Record::new();

        let error = tree
            .set_field_value("items", Value::Array(vec![]), &mut record)
            .unwrap_err();

        assert!(matches!(
            error,
            crate::errors::Error::UnsupportedNestingArray(name) if name == "items"
        ));
    }

    fn field_mapping() -> Vec<Field> {
        vec![
            Field::Single {
                name: FieldName::new(
                    "timestamp".to_owned(),
                    false,
                    None,
                    None,
                    Some("Event generation time".to_owned()),
                ),
                parser: Parser::DateTime(DateInputCodec::Iso()),
                default_value: None,
            },
            Field::Object {
                name: FieldName::new(
                    "System".to_owned(),
                    false,
                    Some("system".to_owned()),
                    None,
                    None,
                ),
                ignore: false,
                fields: vec![
                    Field::Object {
                        name: FieldName::new(
                            "TimeCreated_attributes".to_owned(),
                            false,
                            Some("time_created".to_owned()),
                            None,
                            None,
                        ),
                        ignore: false,
                        fields: vec![Field::Single {
                            name: FieldName::new(
                                "SystemTime".to_owned(),
                                false,
                                Some("system_time".to_owned()),
                                None,
                                Some("Event written time".to_owned()),
                            ),
                            parser: Parser::DateTime(DateInputCodec::Iso()),
                            default_value: None,
                        }],
                    },
                    Field::Single {
                        name: FieldName::new(
                            "EventRecordID".to_owned(),
                            false,
                            Some("event_record_id".to_owned()),
                            None,
                            None,
                        ),
                        parser: Parser::Ignore(),
                        default_value: None,
                    },
                ],
            },
            Field::Object {
                name: FieldName::new(
                    "EventData".to_owned(),
                    false,
                    Some("event_data".to_owned()),
                    None,
                    None,
                ),
                ignore: false,
                fields: vec![],
            },
        ]
    }
}
