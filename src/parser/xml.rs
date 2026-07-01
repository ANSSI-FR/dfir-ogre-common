use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::Read;
use std::{fs::File, path::Path};

use crate::{
    Error, FieldParserTree, Metadata, Output, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration, field_mapping,
};

use pyo3::prelude::*;
use regex::Regex;
use sxd_xpath::XPath;
use sxd_xpath::nodeset::Node;

#[pyfunction]
pub fn parse_xml(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport {
    match parse(input_file, run_config, plugin_config, metadata) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

pub fn parse(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> Result<RunReport, Error> {
    let data_type_config = &plugin_config.get_data_type_mapping(None)?;
    let mut parser_tree = data_type_config
        .field_mapping
        .as_ref()
        .ok_or(Error::ConfigurationError(
            "There is no field mapping in the configuration".to_string(),
        ))?
        .get_field_parser_tree();

    let tuple_xpath =
        data_type_config
            .params
            .get("xpath_tuple")
            .ok_or(Error::ConfigurationError(
                "'xpath_tuple' is not set in the configuration ".to_string(),
            ))?;

    let mut xpath_cache = XPathCache::new();
    let xpath_expr = xpath_cache.build(tuple_xpath)?;

    let mut file_handle = File::open(Path::new(input_file))
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let mut xml_str = String::new();
    file_handle.read_to_string(&mut xml_str)?;
    let xml_str = strip_namespaces(&xml_str);

    let sxd_package = sxd_document::parser::parse(&xml_str)?;
    let xpath_context = sxd_xpath::Context::new();
    let mut run_report = RunReport::new();

    let mut output = Output::new(run_config, plugin_config, metadata, None)?;

    let document = sxd_package.as_document();
    let result = xpath_expr.evaluate(&xpath_context, document.root())?;

    if let sxd_xpath::Value::Nodeset(nodeset) = result {
        for node in nodeset.document_order() {
            let record_opt =
                parse_record(&node, &mut parser_tree, &mut xpath_cache, &xpath_context);

            match record_opt {
                Ok(mut record) => {
                    if let Err(e) = output.write(&mut record) {
                        run_report.add_error(e.to_string());
                    }
                }
                Err(e) => {
                    run_report.add_error(e.to_string());
                }
            };
        }
    }
    run_report.add_output_report(output.get_report());
    Ok(run_report)
}

fn parse_record(
    node: &Node,
    parser_tree: &mut FieldParserTree,
    xpath_cache: &mut XPathCache,
    context: &sxd_xpath::Context,
) -> Result<Record, Error> {
    let mut record = Record::new();
    for (xpath, parser_type) in parser_tree.parsers.iter_mut() {
        let xpath_expr = xpath_cache.build(xpath)?;

        let result = xpath_expr.evaluate(context, *node)?;

        let node_set = if let sxd_xpath::Value::Nodeset(nodeset) = result {
            nodeset
        } else {
            continue;
        };
        match parser_type {
            field_mapping::ParserType::Field { parser } => {
                //handle signle value fields
                match node_set.iter().next() {
                    Some(node) => {
                        parser.parse(Some(&node.string_value()), &mut record)?;
                    }
                    None => parser.parse(None, &mut record)?,
                }
            }
            field_mapping::ParserType::Object { field_parsers } => {
                if let Some(node) = node_set.iter().next() {
                    let object = parse_record(&node, field_parsers, xpath_cache, context)?;
                    let name = field_parsers.field_name.as_ref();
                    if let Some(name) = name {
                        record.add(name.output_name(), Value::Object(object));
                    }
                }
            }
            field_mapping::ParserType::Array { field } => {
                let parser_type = field.0.as_mut();
                match parser_type {
                    field_mapping::ParserType::Field { parser } => {
                        let parser = &mut parser.fields[0];
                        let mut value_array = Vec::with_capacity(node_set.size());
                        for node in node_set {
                            let value = parser.get_value(Some(&node.string_value()))?;
                            if let Some(value) = value {
                                value_array.push(value);
                            }
                        }
                        record.add(parser.output_name(), Value::Array(value_array));
                    }
                    field_mapping::ParserType::Array { field: _ } => {
                        return Err(Error::UnsupportedNestingArray(xpath.to_string()));
                    }
                    field_mapping::ParserType::Object { field_parsers } => {
                        let mut value_array = Vec::with_capacity(node_set.size());
                        for node in node_set {
                            let object = parse_record(&node, field_parsers, xpath_cache, context)?;
                            value_array.push(Value::Object(object));
                        }
                        let name = field_parsers.field_name.as_ref();
                        if let Some(name) = name {
                            record.add(name.output_name(), Value::Array(value_array));
                        }
                    }
                }
            }
        }
    }
    Ok(record)
}

struct XPathCache {
    map: HashMap<String, XPath>,
    factory: sxd_xpath::Factory,
}
impl XPathCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            factory: sxd_xpath::Factory::new(),
        }
    }

    fn build(&mut self, expression: &str) -> Result<&XPath, Error> {
        let xpath = self.map.entry(expression.to_string());
        if let Entry::Vacant(vacant) = xpath {
            let xpath_expr = self
                .factory
                .build(expression)?
                .ok_or(Error::ConfigurationError(format!(
                    "failed to compile Xpath expression:'{expression}'"
                )))?;
            vacant.insert(xpath_expr);
        }

        Ok(self.map.get(expression).unwrap())
    }
}

///
///  removes `xmlns` namespaces from an XML string to allow simple xpath queries
///
fn strip_namespaces(xml: &str) -> String {
    let re_xmlns =
        Regex::new("\\s+xmlns(?::\\w+)?\\s*=\\s*\"[^\"]*\"").expect("this regexp should compile");
    let xml = re_xmlns.replace_all(xml, "");
    xml.into_owned()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs};

    use crate::OutputConfiguration;

    use super::*;

    #[test]
    fn xml_with_configuration() {
        let output_folder = ".tmp";
        let base_file_name = "xml_with_configuration";
        let targetfile = format!("{output_folder}/{base_file_name}.library.jsonl");
        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            output_folder.to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            true,
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);
        let xml = fs::read_to_string("test_data/xml/library_config.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let metadata = Metadata::new("test".into());

        let report = parse_xml(
            "test_data/xml/library.xml",
            configuration,
            plugin_config,
            metadata,
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(2, lines.len());

        let line = lines[0].as_object().unwrap();
        let user = line.get("id").unwrap().as_str().unwrap();

        assert_eq!(user, "b1");
    }

    #[test]
    fn xpath_without_namespace() {
        let xml = fs::read_to_string("test_data/xml/FullCompatReport.xml").unwrap();
        let xml = strip_namespaces(&xml);
        let sxd_package = sxd_document::parser::parse(&xml).unwrap();

        let xpath_context = sxd_xpath::Context::new();
        let factory = sxd_xpath::Factory::new();
        let xpath = "/CompatReport/Programs/Program";
        let xpath_expr = factory.build(xpath).unwrap().unwrap();

        let document = sxd_package.as_document();

        let result = xpath_expr
            .evaluate(&xpath_context, document.root())
            .unwrap();
        if let sxd_xpath::Value::Nodeset(nodeset) = result {
            assert_eq!(nodeset.size(), 4);
        }
    }

    #[test]
    fn test_strip_namespaces_multiple_spaces() {
        // Ensure that the leading whitespace before the attribute is also removed
        let input = r#"<elem    xmlns ="uri"   xmlns:ns="uri2"   attr="v"/> "#;
        let expected = r#"<elem   attr="v"/> "#;
        let result = strip_namespaces(input);
        assert_eq!(result, expected);
    }
}
