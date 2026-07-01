use std::{collections::HashMap, fs};

use crate::{
    DateInputCodec, Error, Field, FieldMapping, FieldName, FieldParserTree, MultiInputField,
    MultiParser, Parser, TimeLineBuilder, TimeLineType,
    field::{ArrayField, ParserExtension, PyParser},
    timeline::{ConditionalDescriptionConf, TimelineDisplayOptions},
};
use encoding_rs::*;
use encoding_rs_io::DecodeReaderBytesBuilder;
use pyo3::{prelude::*, types::PyType};
use xmltree::Element;

/// Configuration for a plugin, containing the parser name, file encoding, and data‑type mappings.
#[derive(Default, Clone)]
#[pyclass(get_all, from_py_object)]
pub struct PluginConfiguration {
    /// Name of the parser associated with this plugin.
    pub plugin: String,
    /// Encoding used for plugin files (e.g., `"UTF_8"`).
    pub file_encoding: String,
    /// List of data‑type mappings defined by the plugin.
    pub data_type_configs: Vec<DataTypeMapping>,
}

impl PluginConfiguration {
    /// Build a `PluginConfiguration` from an XML string.
    ///
    /// * `xml` – The raw XML containing the plugin definition.
    /// * `python` – Optional map of Python parser extensions.
    /// * `extension` – Optional map of Rust parser extensions.
    ///
    /// Returns an error if the XML cannot be parsed or required nodes are missing.
    pub fn from_str(
        xml: &str,
        python: Option<HashMap<String, Py<PyAny>>>,
        extension: Option<HashMap<String, ParserExtension>>,
    ) -> Result<Self, Error> {
        let root = Element::parse(xml.as_bytes())?;
        if !root.name.eq("plugin") {
            return Err(Error::ConfigurationError(format!(
                "The root node must be 'plugin'. found '{}' ",
                root.name
            )));
        }

        // The top‑level `<plugin>` node must contain a `parser` attribute.
        let plugin = attribute("parser", &root)?;
        let file_encoding = root
            .attributes
            .get("file_encoding")
            .cloned()
            .unwrap_or("UTF_8".to_string());

        let mut mappings = vec![];

        let python = python.unwrap_or_default();
        let extension = extension.unwrap_or_default();

        for xml_node in root.children {
            if let Some(element) = xml_node.as_element()
                && element.name.eq("mapping")
            {
                let mapping = parse_mapping(element, &python, &extension)?;
                mappings.push(mapping);
            }
        }

        if mappings.is_empty() {
            return Err(Error::ConfigurationError(
                "plugin must have at least one DataTypeMapping".to_string(),
            ));
        }
        Ok(Self {
            plugin,
            file_encoding,
            data_type_configs: mappings,
        })
    }
}

#[pymethods]
impl PluginConfiguration {
    #[classmethod]
    /// Load a plugin configuration from an XML file (Python binding).
    ///
    /// * `input_file` – Path to the XML file defining the plugin.
    /// * `python` – Optional Python parser extensions.
    /// * `extension` – Optional Rust parser extensions.
    ///
    #[pyo3(signature = (input_file, python=None, extension=None))]
    pub fn load(
        _cls: &Bound<'_, PyType>,
        input_file: &str,
        python: Option<HashMap<String, Py<PyAny>>>,
        extension: Option<HashMap<String, ParserExtension>>,
    ) -> Result<PluginConfiguration, Error> {
        let xml_string = fs::read_to_string(input_file)
            .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

        PluginConfiguration::from_str(&xml_string, python, extension)
    }

    /// Returns the field parser tree for the specified data type.
    ///
    /// If `data_type` is `None`, the parser tree for the first configured data type is returned.
    ///
    /// * `data_type` – Optional string specifying the data type to look up.
    #[pyo3(signature = (data_type=None))]
    pub fn get_parsers(&self, data_type: Option<String>) -> Option<FieldParserTree> {
        let data_type_mapping = match data_type {
            Some(dtype) => self.data_type_configs.iter().find(|c| c.data_type == dtype),
            None => self.data_type_configs.first(),
        };

        data_type_mapping
            .and_then(|mapping| mapping.field_mapping.as_ref())
            .map(|fm| fm.get_field_parser_tree())
    }

    /// Returns the DataTypeMapping for the specified data type.
    ///
    /// If `data_type` is `None`, the parser tree for the first configured data type is returned.
    ///
    /// * `data_type` – Optional string specifying the data type to look up.
    #[pyo3(signature = (data_type=None))]
    pub fn get_data_type_mapping(
        &self,
        data_type: Option<String>,
    ) -> Result<DataTypeMapping, Error> {
        match data_type {
            Some(dtype) => {
                let datatype = self.data_type_configs.iter().find(|c| c.data_type == dtype);
                datatype
                    .cloned()
                    .ok_or(Error::UnknownDataTypeMapping(dtype))
            }
            None => self
                .data_type_configs
                .first()
                .cloned()
                .ok_or(Error::UnknownDataTypeMapping("Default".to_string())),
        }
    }
}
/// Mapping of a specific data type inside a plugin configuration.
#[derive(Clone)]
#[pyclass(get_all, from_py_object)]
pub struct DataTypeMapping {
    /// The name of the data type this mapping describes.
    pub data_type: String,
    /// Optional human‑readable description.
    pub description: Option<String>,
    /// Date‑parsing codec used when no explicit pattern is provided.
    pub default_date_pattern: DateInputCodec,
    /// Arbitrary key‑value parameters for the mapping.
    pub params: HashMap<String, String>,
    /// Optional timeline definition for this data type.
    pub timeline: Option<TimeLineBuilder>,
    /// Field mapping that ties XML fields to parsers.
    pub field_mapping: Option<FieldMapping>,
    ///indicate whether some fields are tagged with the primary key flag. it is used to compute unique ids
    pub has_primary_key: bool,
}

fn parse_mapping(
    node: &Element,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
) -> Result<DataTypeMapping, Error> {
    let data_type = attribute("data_type", node)?;

    let mut config = DataTypeMapping {
        data_type,
        description: None,
        default_date_pattern: DateInputCodec::Iso(),
        params: HashMap::new(),
        timeline: None,
        field_mapping: None,
        has_primary_key: false,
    };
    let mut fields = vec![];
    let mut default_parser = None;
    let mut contains_primary_key = false;

    for xml_node in &node.children {
        if let Some(element) = xml_node.as_element() {
            match element.name.as_str() {
                "timeline" => {
                    let timeline =
                        parse_timeline(element, &config.data_type, &config.default_date_pattern)?;
                    config.timeline = Some(timeline)
                }

                "fields" => {
                    for node in &element.children {
                        if let Some(elem) = node.as_element() {
                            let field = parse_field_node(
                                elem,
                                &config.default_date_pattern,
                                python,
                                extension,
                                &mut contains_primary_key,
                            )?;
                            fields.push(field);
                        }
                    }
                }
                "description" => {
                    let value = element.get_text().map(|data| data.to_string());
                    config.description = value;
                }
                "default_parser" => {
                    let value = element
                        .attributes
                        .get("value")
                        .cloned()
                        .unwrap_or("Ignore".to_string());

                    let parser = match value.as_str() {
                        "String" => Parser::String(),
                        "Ignore" => Parser::Ignore(),
                        _ => {
                            return Err(Error::ConfigurationError(format!(
                                "default parser can be either: 'String' or 'Ignore'. Founf: '{value}'"
                            )));
                        }
                    };
                    default_parser = Some(parser);
                }
                "default_date_pattern" => {
                    let value = element
                        .attributes
                        .get("value")
                        .cloned()
                        .unwrap_or("iso".to_string());

                    let codec = DateInputCodec::from_str(&value);
                    config.default_date_pattern = codec;
                }
                _ => {
                    let value = match element.attributes.get("value") {
                        Some(val) => val.to_string(),
                        None => element
                            .get_text()
                            .map(|data| data.trim().to_string())
                            .unwrap_or("".to_string()),
                    };

                    config.params.insert(element.name.to_string(), value);
                }
            }
        }
    }
    config.has_primary_key = contains_primary_key;
    config.field_mapping = Some(FieldMapping::new(fields, default_parser));
    Ok(config)
}

fn parse_timeline(
    node: &Element,
    data_type: &str,
    default_date_pattern: &DateInputCodec,
) -> Result<TimeLineBuilder, Error> {
    let timeline_type_elem = child("timeline_type", node)?;

    let timeline_type = attribute("value", timeline_type_elem)?;
    let timeline_type = match timeline_type.as_ref() {
        "Standard" => TimeLineType::Standard,
        "MacbMacb" => TimeLineType::MacbMacb,
        _ => {
            return Err(Error::ConfigurationError(format!(
                "timeline type support either 'Standard' or 'MacbMacb' values. Found: '{timeline_type}'"
            )));
        }
    };

    let mut timeline_builder =
        TimeLineBuilder::new(timeline_type, data_type.to_string(), 0, None, None);

    let related_user = match node.get_child("related_user") {
        Some(desc) => parse_timeline_desc(desc)?,
        None => vec![],
    };
    for user_field in related_user {
        let desc_array = user_field.split(".").map(|s| s.to_string()).collect();
        timeline_builder.add_related_user_ouput_path(desc_array);
    }

    let description_elem = child("description", node)?;
    let description_display = parse_display_options(description_elem);
    timeline_builder.description_format = description_display;
    let descriptions = parse_timeline_desc(description_elem)?;
    for description in descriptions {
        let desc_array = description.split(".").map(|s| s.to_string()).collect();
        timeline_builder.add_description_ouput_path(desc_array);
    }

    if let Some(desc) = node.get_child("additional_description") {
        let display_option = parse_display_options(desc);
        timeline_builder.additional_description_format = display_option;
        parse_timeline_additional_descr(desc, &mut timeline_builder, default_date_pattern)?;
    }

    Ok(timeline_builder)
}

fn parse_display_options(description_elem: &Element) -> TimelineDisplayOptions {
    let include_field_name = description_elem
        .attributes
        .get("include_field_name")
        .cloned()
        .unwrap_or("true".to_string());

    let include_field_name: bool = include_field_name.parse().unwrap_or(true);

    let field_separator = description_elem
        .attributes
        .get("field_separator")
        .cloned()
        .unwrap_or(" - ".to_string());

    TimelineDisplayOptions::new(include_field_name, field_separator)
}

fn parse_timeline_desc(description_node: &Element) -> Result<Vec<String>, Error> {
    let mut desc = Vec::with_capacity(description_node.children.len());
    for xml_node in &description_node.children {
        if let Some(elem) = xml_node.as_element() {
            if elem.name.eq("output_name") {
                desc.push(attribute("value", elem)?);
            } else {
                return Err(Error::ConfigurationError(format!(
                    "timeline description only supports  'output_name' nodes. Found: '{}'",
                    elem.name
                )));
            }
        }
    }
    Ok(desc)
}

fn parse_timeline_additional_descr(
    description_node: &Element,
    timeline_builder: &mut TimeLineBuilder,
    default_date_pattern: &DateInputCodec,
) -> Result<(), Error> {
    for xml_node in &description_node.children {
        if let Some(elem) = xml_node.as_element() {
            if elem.name.eq("output_name") {
                let path: Vec<String> = attribute("value", elem)?
                    .split(".")
                    .map(|s| s.to_string())
                    .collect();
                timeline_builder.add_additional_description_ouput_path(path);
            } else if elem.name.eq("conditional") {
                parse_timeline_conditional_descr(elem, timeline_builder, default_date_pattern)?;
            } else if elem.name.eq("otherwise") {
                parse_timeline_otherwise_descr(elem, timeline_builder)?;
            } else {
                return Err(Error::ConfigurationError(format!(
                    "timeline description only supports  'output_name', 'conditional', and 'otherwise' nodes. Found: '{}'",
                    elem.name
                )));
            }
        }
    }
    Ok(())
}

fn parse_timeline_conditional_descr(
    conditional_node: &Element,
    timeline_builder: &mut TimeLineBuilder,
    default_date_pattern: &DateInputCodec,
) -> Result<(), Error> {
    let mut configs = vec![];
    configs.push(ConditionalDescriptionConf::new());
    for xml_node in &conditional_node.children {
        if let Some(elem) = xml_node.as_element() {
            if elem.name.eq("output_name") {
                let path: Vec<String> = attribute("value", elem)?
                    .split(".")
                    .map(|s| s.to_string())
                    .collect();
                for conf in configs.iter_mut() {
                    conf.add_optional_field(path.clone());
                }
            } else if elem.name.eq("condition") {
                let full_path = attribute("path", elem)?;
                let path: Vec<String> = full_path.split(".").map(|s| s.to_string()).collect();
                let parser_name = attribute("parser", elem)?;

                let value_str = attribute("value", elem)?;
                let values_str = value_str.split("|");
                let mut values = Vec::new();
                for value_str in values_str {
                    let parser = get_parser(
                        elem,
                        &format!("condition: {full_path}"),
                        &parser_name,
                        default_date_pattern,
                        &HashMap::new(),
                        &HashMap::new(),
                    )?;
                    let value = parser
                    .get_value(Some(value_str))?
                    .ok_or(Error::ConfigurationError(format!("No result while parsing condition expression value '{value_str}' for path:'{full_path}'. ")))?;
                    values.push(value);
                }

                if let Some(value) = values.pop() {
                    //update existing conf with the first value
                    for config in configs.iter_mut() {
                        config.add_condition(path.clone(), value.clone());
                    }
                    //then duplicate existing configuration and update them with other values
                    let mut new_configs = vec![];
                    for value in values {
                        for config in configs.iter_mut() {
                            let mut new_conf = config.clone();
                            new_conf.add_condition(path.clone(), value.clone());
                            new_configs.push(new_conf);
                        }
                    }
                    configs.append(&mut new_configs);
                }
            } else {
                return Err(Error::ConfigurationError(format!(
                    "timeline conditional description only supports  'output_name' and 'condition' nodes. Found: '{}'",
                    elem.name
                )));
            }
        }
    }
    for config in configs {
        timeline_builder.add_conditional_description(config);
    }

    Ok(())
}

fn parse_timeline_otherwise_descr(
    conditional_node: &Element,
    timeline_builder: &mut TimeLineBuilder,
) -> Result<(), Error> {
    for xml_node in &conditional_node.children {
        if let Some(elem) = xml_node.as_element()
            && elem.name.eq("output_name")
        {
            let path: Vec<String> = attribute("value", elem)?
                .split(".")
                .map(|s| s.to_string())
                .collect();
            timeline_builder.add_otherwise_description_path(path);
        }
    }
    Ok(())
}

fn parse_field_node(
    elem: &Element,
    default_date_pattern: &DateInputCodec,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
    contains_primary_key: &mut bool,
) -> Result<Field, Error> {
    match elem.name.as_str() {
        "field" => parse_field(
            elem,
            default_date_pattern,
            python,
            extension,
            contains_primary_key,
        ),
        "array" => parse_array(
            elem,
            default_date_pattern,
            python,
            extension,
            contains_primary_key,
        ),
        "object" => parse_object(
            elem,
            default_date_pattern,
            python,
            extension,
            contains_primary_key,
        ),
        "multi_input" => parse_multi_input(
            elem,
            default_date_pattern,
            python,
            extension,
            contains_primary_key,
        ),
        _ => Err(Error::ConfigurationError(format!(
            "field can be either  'field', 'array', 'multi_input', or 'object'. Found: '{}'",
            elem.name
        ))),
    }
}

fn parse_field(
    elem: &Element,
    default_date_pattern: &DateInputCodec,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
    contains_primary_key: &mut bool,
) -> Result<Field, Error> {
    let name = parse_field_name(elem)?;
    if name.primary_key {
        *contains_primary_key = true;
    }
    let parser_name = attribute("parser", elem)?;
    let default_value = elem.attributes.get("default_value").cloned();

    let parser = get_parser(
        elem,
        name.input_name(),
        &parser_name,
        default_date_pattern,
        python,
        extension,
    )?;

    Ok(Field::Single {
        name,
        parser,
        default_value,
    })
}

fn parse_field_name(elem: &Element) -> Result<FieldName, Error> {
    let attributes = &elem.attributes;
    let input_name = attribute("input", elem)?;
    let primary_key = attributes.get("primary_key").cloned();

    let primary_key = primary_key.is_some();

    let output_name = attributes.get("output").cloned();

    let display_name = attributes.get("display_name").cloned();

    let description = attributes.get("description").cloned();
    Ok(FieldName::new(
        input_name,
        primary_key,
        output_name,
        None,
        display_name,
        description,
    ))
}

fn parse_object(
    elem: &Element,
    default_date_pattern: &DateInputCodec,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
    contains_primary_key: &mut bool,
) -> Result<Field, Error> {
    let attributes = &elem.attributes;
    let field_name = parse_field_name(elem)?;
    let mut fields = Vec::with_capacity(elem.children.len());
    for node in &elem.children {
        if let Some(elem) = node.as_element() {
            let field = parse_field_node(
                elem,
                default_date_pattern,
                python,
                extension,
                contains_primary_key,
            )?;
            fields.push(field);
        }
    }
    let ignore_str = attributes
        .get("ignore")
        .cloned()
        .unwrap_or("false".to_string());

    let ignore = ignore_str.parse()?;

    Ok(Field::Object {
        name: field_name,
        fields,
        ignore,
    })
}

fn parse_array(
    elem: &Element,
    default_date_pattern: &DateInputCodec,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
    contains_primary_key: &mut bool,
) -> Result<Field, Error> {
    let childrens: Vec<&Element> = elem
        .children
        .iter()
        .filter_map(|node| node.as_element())
        .collect();

    if childrens.len() != 1 {
        let children_names: Vec<String> =
            childrens.iter().map(|elem| elem.name.to_string()).collect();
        return Err(Error::ConfigurationError(format!(
            "Array definition requires exactly *one* field, found '{}' fields. children nodes: {}",
            childrens.len(),
            children_names.join("\n")
        )));
    }
    let elem = childrens[0];

    let field = parse_field_node(
        elem,
        default_date_pattern,
        python,
        extension,
        contains_primary_key,
    )?;

    Ok(Field::Array(ArrayField::new(field)))
}

fn parse_multi_input(
    elem: &Element,
    default_date_pattern: &DateInputCodec,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
    contains_primary_key: &mut bool,
) -> Result<Field, Error> {
    let attributes = &elem.attributes;
    let output_name = attribute("output", elem)?;
    let display_name = attributes.get("display_name").cloned();
    let description = attributes.get("description").cloned();
    let output_field = FieldName::new(
        output_name.clone(),
        false,
        Some(output_name),
        None,
        display_name,
        description,
    );
    let parser = attribute("parser", elem)?;
    let parser = match parser.as_str() {
        "Join" => {
            let separator = attribute("separator", elem)?;
            let avoid_separator_duplication = attributes
                .get("avoid_separator_duplication")
                .cloned()
                .unwrap_or("true".to_string());
            let avoid_separator_duplication = avoid_separator_duplication.parse()?;
            MultiParser::Join(separator, avoid_separator_duplication)
        }
        _ => {
            return Err(Error::ConfigurationError(format!(
                "Invalid MultiParser value '{parser}'"
            )));
        }
    };
    let mut input_fields = Vec::with_capacity(elem.children.len());
    for node in &elem.children {
        if let Some(elem) = node.as_element() {
            let field = parse_field_node(
                elem,
                default_date_pattern,
                python,
                extension,
                contains_primary_key,
            )?;
            input_fields.push(field);
        }
    }

    Ok(Field::Multi(MultiInputField::new(
        input_fields,
        output_field,
        parser,
    )))
}

fn get_parser(
    elem: &Element,
    input_name: &str,
    parser_name: &str,
    default_date_pattern: &DateInputCodec,
    python: &HashMap<String, Py<PyAny>>,
    extension: &HashMap<String, ParserExtension>,
) -> Result<Parser, Error> {
    let parser = match parser_name {
        "Ignore" => Parser::Ignore(),
        "Int" => Parser::Int(),
        "Float" => Parser::Float(),
        "Bool" => Parser::Bool(),
        "String" => Parser::String(),
        "StringToUpper" => Parser::StringToUpper(),
        "StringToLower" => Parser::StringToLower(),
        "IntRadix" => {
            let radix = elem.attributes.get("radix").cloned();
            match radix {
                Some(s) => match s.parse() {
                    Ok(radix) => Parser::IntRadix(radix),
                    Err(_) => {
                        return Err(Error::ConfigurationError(format!(
                            "Invalid radix value '{s}' it expects a valid unsigned integer. Field: '{}'",
                            input_name
                        )));
                    }
                },
                None => {
                    return Err(Error::ConfigurationError(format!(
                        "'IntRadix' requires a radix integer attribute. Field: '{}'",
                        input_name
                    )));
                }
            }
        }
        "IntToHex" => {
            let width = elem
                .attributes
                .get("width")
                .cloned()
                .unwrap_or("0".to_string());

            match width.parse() {
                Ok(width) => Parser::IntToHex(width),
                Err(_) => {
                    return Err(Error::ConfigurationError(format!(
                        "Invalid width value '{width}' it expects a valid unsigned integer. Field: '{}'",
                        input_name
                    )));
                }
            }
        }
        "Dynamic" => {
            let mut parsers = Vec::with_capacity(elem.children.len());
            for xml_node in &elem.children {
                if let Some(elem) = xml_node.as_element() {
                    if elem.name.eq("parser") {
                        let parser_name = attribute("value", elem)?;
                        let parser = get_parser(
                            elem,
                            input_name,
                            &parser_name,
                            default_date_pattern,
                            python,
                            extension,
                        )?;
                        parsers.push(parser);
                    } else {
                        return Err(Error::ConfigurationError(format!(
                            "dynamic parsers definition only supports 'parser' nodes. Found: '{}'",
                            elem.name
                        )));
                    }
                }
            }
            Parser::Dynamic(parsers)
        }

        "DateTime" => {
            let codec = match elem.attributes.get("date_codec") {
                Some(date_codec) => DateInputCodec::from_str(date_codec),
                None => default_date_pattern.clone(),
            };
            Parser::DateTime(codec)
        }
        "Split" => {
            let split = elem.attributes.get("split_by").cloned();
            match split {
                Some(s) => Parser::Split(s),
                None => {
                    return Err(Error::ConfigurationError(format!(
                        "'Split' parser requires a split_by attribute. Field: '{}'",
                        input_name
                    )));
                }
            }
        }
        "Python" => {
            if let Some(p) = python.get(input_name) {
                Python::attach(|py| {
                    let parser = PyParser::new(p.clone_ref(py));
                    Parser::Python(parser)
                })
            } else {
                return Err(Error::UnknownPythonParser(input_name.to_string()));
            }
        }
        "Extension" => {
            if let Some(parser) = extension.get(input_name) {
                Parser::Extension(parser.clone())
            } else {
                return Err(Error::UnknownParserExtension(input_name.to_string()));
            }
        }
        _ => {
            return Err(Error::ConfigurationError(format!(
                "Invalid parser: '{parser_name}' for field: '{}'",
                input_name
            )));
        }
    };
    Ok(parser)
}

fn child<'a>(key: &str, node: &'a Element) -> Result<&'a Element, Error> {
    match node.get_child(key) {
        Some(val) => Ok(val),
        None => Err(Error::ConfigurationError(format!(
            "child '{key}' not found for node: '{}' ",
            node.name
        ))),
    }
}

fn attribute(key: &str, node: &Element) -> Result<String, Error> {
    match node.attributes.get(key) {
        Some(val) => Ok(val.to_string()),
        None => Err(Error::ConfigurationError(format!(
            "attribute '{key}' not found for node: '{}' ",
            node.name
        ))),
    }
}

/// Build a `DecodeReaderBytesBuilder` for the given encoding label.
///
/// The `value` argument should match one of the supported encoding names
/// (e.g., `"UTF_8"`, `"WINDOWS_1252"`). If the name is unknown, an
/// `Error::ConfigurationError` is returned.
///
/// This helper isolates the `encoding_rs` lookup logic from the rest of
/// the codebase, keeping the caller focused on reading plugin files.
pub fn encoding_reader_builder(value: &str) -> Result<DecodeReaderBytesBuilder, Error> {
    let encoding = match value {
        "UTF_8" => Some(UTF_8),
        "UTF_16_BE" => Some(UTF_16BE),
        "UTF_16_LE" => Some(UTF_16LE),
        "WINDOWS_1250" => Some(WINDOWS_1250),
        "WINDOWS_1251" => Some(WINDOWS_1251),
        "WINDOWS_1252" => Some(WINDOWS_1252),
        "WINDOWS_1253" => Some(WINDOWS_1253),
        "WINDOWS_1254" => Some(WINDOWS_1254),
        "WINDOWS_1255" => Some(WINDOWS_1255),
        "WINDOWS_1256" => Some(WINDOWS_1256),
        "WINDOWS_1257" => Some(WINDOWS_1257),
        "WINDOWS_1258" => Some(WINDOWS_1258),
        "ISO_2022_JP" => Some(ISO_2022_JP),
        "ISO_8859_2" => Some(ISO_8859_2),
        "ISO_8859_3" => Some(ISO_8859_3),
        "ISO_8859_4" => Some(ISO_8859_4),
        "ISO_8859_5" => Some(ISO_8859_5),
        "ISO_8859_6" => Some(ISO_8859_6),
        "ISO_8859_7" => Some(ISO_8859_7),
        "ISO_8859_8" => Some(ISO_8859_8),
        "ISO_8859_10" => Some(ISO_8859_10),
        "ISO_8859_13" => Some(ISO_8859_13),
        "ISO_8859_14" => Some(ISO_8859_14),
        "ISO_8859_15" => Some(ISO_8859_15),
        "IBM866" => Some(IBM866),
        _ => {
            return Err(Error::ConfigurationError(format!(
                "Encoding '{value}' is not supported ",
            )));
        }
    };
    let mut builder = DecodeReaderBytesBuilder::new();
    builder.encoding(encoding);
    Ok(builder)
}

#[cfg(test)]
mod tests {

    use crate::win_frn_hex_parser;

    use super::*;

    fn configuration_error(data: &str) -> String {
        match PluginConfiguration::from_str(data, None, None) {
            Err(Error::ConfigurationError(message)) => message,
            Err(error) => panic!("unexpected error: {error}"),
            Ok(_) => panic!("configuration unexpectedly parsed"),
        }
    }

    #[test]
    fn from_str_rejects_non_plugin_root() {
        let message = configuration_error(
            r#"<?xml version="1.0" encoding="UTF-8"?><not_plugin parser="Test" />"#,
        );

        assert!(message.contains("root node must be 'plugin'"));
    }

    #[test]
    fn from_str_requires_parser_attribute() {
        let message = configuration_error(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin>
   <mapping data_type="test">
      <fields />
   </mapping>
</plugin>"#,
        );

        assert!(message.contains("attribute 'parser' not found"));
    }

    #[test]
    fn from_str_rejects_plugin_without_mappings() {
        let message = configuration_error(
            r#"<?xml version="1.0" encoding="UTF-8"?><plugin parser="Test" />"#,
        );

        assert!(message.contains("at least one DataTypeMapping"));
    }

    #[test]
    fn from_str_accepts_free_form_qualifier_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <fields>
         <field input="Known" output="known" parser="String" qualifier="DATE_CREATION" />
         <object input="Container" output="container" qualifier="ANY_OBJECT_LABEL">
            <field input="Child" output="child" parser="String" qualifier="ANY_CHILD_LABEL" />
         </object>
         <multi_input output="joined" qualifier="ANY_MULTI_LABEL" parser="Join" separator="-">
            <field input="First" parser="String" qualifier="ANY_FIRST_LABEL" />
            <field input="Second" parser="String" qualifier="ANY_SECOND_LABEL" />
         </multi_input>
      </fields>
   </mapping>
</plugin>"#;

        let config = PluginConfiguration::from_str(xml, None, None).unwrap();
        let mapping = config.data_type_configs[0]
            .field_mapping
            .as_ref()
            .expect("field mapping");

        let output_fields = &mapping.field_parser_tree.output_fields;
        assert_eq!(output_fields.len(), 3);
        assert_eq!(output_fields[0].output_name(), "known");
        assert_eq!(output_fields[1].output_name(), "container");
        assert_eq!(output_fields[2].output_name(), "joined");
    }

    #[test]
    fn mapping_preserves_params_and_default_ignore_parser() {
        let data = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <default_parser value="Ignore" />
      <csv_delimiter value="|" />
      <text_param>
         padded text
      </text_param>
      <fields />
   </mapping>
</plugin>"#;
        let config = PluginConfiguration::from_str(data, None, None).unwrap();
        let mapping = &config.data_type_configs[0];

        assert_eq!(mapping.params.get("csv_delimiter").unwrap(), "|");
        assert_eq!(mapping.params.get("text_param").unwrap(), "padded text");

        let mut parser_tree = mapping
            .field_mapping
            .as_ref()
            .unwrap()
            .get_field_parser_tree();
        let parser = parser_tree.get_parser("unknown").unwrap();
        let mut record = crate::Record::new();
        parser.parse(Some("ignored"), &mut record).unwrap();
        assert!(record.is_empty());
    }

    #[test]
    fn array_definition_requires_exactly_one_child_field() {
        let message = configuration_error(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <fields>
         <array>
            <field input="first" parser="String" />
            <field input="second" parser="String" />
         </array>
      </fields>
   </mapping>
</plugin>"#,
        );

        assert!(message.contains("Array definition requires exactly *one* field"));
    }

    #[test]
    fn invalid_default_parser_is_rejected() {
        let message = configuration_error(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <default_parser value="Int" />
      <fields />
   </mapping>
</plugin>"#,
        );

        assert!(message.contains("default parser can be either"));
    }

    #[test]
    fn test_basic_parsers() {
        let data = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <default_parser>String</default_parser>
      <default_date_pattern>iso</default_date_pattern>
      <csv_delimiter>,</csv_delimiter>
      <fields>
         <field input="ignore_field" parser="Ignore"  />
         <field input="sString" parser="String" />
         <field input="anInt" parser="Int" />
         <field input="radixInt" parser="IntRadix" radix="16" />
         <field input="aFloat" parser="Float"  />
         <field input="sString" parser="String" />
         <field input="anothersString" parser="StringToUpper"  />
         <field input="yetAnothersString" parser="StringToLower"  />
      </fields>
   </mapping>
</plugin>
        "#;
        PluginConfiguration::from_str(data, None, None).unwrap();
    }

    #[test]
    fn test_dynamic_parsers() {
        let data = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
   <mapping data_type="test">
      <default_parser>String</default_parser>
      <default_date_pattern>iso</default_date_pattern>
      <csv_delimiter>,</csv_delimiter>
      <fields>
         <field input="ignore_field" parser="Dynamic">
            <parser value="Int" />
            <parser value="Float" />
            <parser value="String" />
         </field>
      </fields>
   </mapping>
</plugin>
        "#;
        PluginConfiguration::from_str(data, None, None).unwrap();
    }

    #[test]
    fn test_extension() {
        let mut extension = HashMap::new();
        extension.insert(
            "droid_file_mft_seq".to_string(),
            win_frn_hex_parser("droid_"),
        );
        extension.insert(
            "birth_droid_file_mft_seq".to_string(),
            win_frn_hex_parser("birth_droid_"),
        );
        let data = std::fs::read_to_string("test_data/config/lnk.xml").unwrap();

        PluginConfiguration::from_str(&data, None, Some(extension)).unwrap();
    }

    #[test]
    fn test_timeline() {
        let data = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
    <mapping data_type="test">
        <default_parser>String</default_parser>
        <default_date_pattern>iso</default_date_pattern>

        <timeline>
            <timeline_type value="Standard" />
            <max_date_meaning value="2" />
            <source_type value="file" />
            <related_user>
                <output_name value="system.security.user_id" />
            </related_user>
            <description include_field_name="false" field_separator=":">
                <output_name value="system.provider.provider_name" />
                <output_name value="system.event_id" />
            </description>
            <additional_description>
                <conditional>
                    <condition path="system.event_id" value="123" parser="Int"/>
                    <condition path="system.provider.provider_name" value="provider" parser="String"/>
                    <output_name value="system.computer" />
                    <output_name value="system.event_record_id" />
                </conditional>
                <conditional>
                    <condition path="system.event_id" value="456" parser="Int"/>
                    <condition path="system.provider.provider_name" value="provider" parser="String"/>
                    <output_name value="system.computer" />
                    <output_name value="system.event_record_id" />
                </conditional>
            </additional_description>
        </timeline>

        <csv_delimiter>,</csv_delimiter>
        <fields>
            <field input="ignore_field" parser="Ignore"  />
        </fields>
    </mapping>
</plugin>
        "#;
        let config = PluginConfiguration::from_str(data, None, None).unwrap();
        let config = &config.data_type_configs[0];
        let timeline = config.timeline.as_ref().unwrap();
        assert_eq!(2, timeline.conditional_fields.len())
    }

    #[test]
    fn test_timeline_muticondition() {
        let data = r#"<?xml version="1.0" encoding="UTF-8"?>
<plugin parser="Test">
    <mapping data_type="test">
        <default_parser>String</default_parser>
        <default_date_pattern>iso</default_date_pattern>

        <timeline>
            <timeline_type value="Standard" />
            <max_date_meaning value="2" />
            <source_type value="file" />
            <related_user>
                <output_name value="system.security.user_id" />
            </related_user>
            <description include_field_name="false" field_separator=":">
                <output_name value="system.provider.provider_name" />
                <output_name value="system.event_id" />
            </description>
            <additional_description>
                <conditional>
                    <condition path="system.event_id" value="123|421|1433" parser="Int"/>
                    <condition path="system.provider.provider_name" value="provider1|provider2" parser="String"/>
                    <output_name value="system.computer" />
                    <output_name value="system.event_record_id" />
                </conditional>

            </additional_description>
        </timeline>

        <csv_delimiter>,</csv_delimiter>
        <fields>
            <field input="ignore_field" parser="Ignore"  />
        </fields>
    </mapping>
</plugin>
        "#;
        let config = PluginConfiguration::from_str(data, None, None).unwrap();
        let config = &config.data_type_configs[0];
        let timeline = config.timeline.as_ref().unwrap();
        assert_eq!(6, timeline.conditional_fields.len());
    }
}
