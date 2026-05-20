use std::io::{BufRead, BufReader, Read};
use std::{fs::File, path::Path};

use crate::field_mapping::ParserType;
use crate::{
    Error, FieldParserTree, Metadata, Output, Parser, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration,
};

use pyo3::prelude::*;
use serde_json::{Map, Value as JsonValue};

#[pyfunction]
pub fn parse_json(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport {
    match parse_json_internal(input_file, run_config, plugin_config, metadata) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

#[pyfunction]
pub fn parse_jsonl(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport {
    match parse_jsonl_internal(input_file, run_config, plugin_config, metadata) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

fn parse_json_internal(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> Result<RunReport, Error> {
    let mut file_handle = File::open(Path::new(input_file))
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let mut jon_str = String::new();
    file_handle.read_to_string(&mut jon_str)?;

    let json: JsonValue = serde_json::from_str(&jon_str)?;

    let mut report = RunReport::new();
    let data_type_config = &plugin_config.get_data_type_mapping(None)?;
    let mut parser_tree = data_type_config
        .field_mapping
        .as_ref()
        .ok_or(Error::ConfigurationError(format!(
            "There is no field mapping in the configuration"
        )))?
        .get_field_parser_tree();
    let mut output = Output::new(run_config, plugin_config, metadata, None)?;

    parse(
        json,
        &mut Record::new(),
        &mut output,
        &mut parser_tree,
        &mut report,
    );

    report.add_output_report(output.get_report());
    Ok(report)
}

fn parse_jsonl_internal(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> Result<RunReport, Error> {
    let data_type_config = &plugin_config.data_type_configs[0];
    let mut parser_tree = data_type_config
        .field_mapping
        .as_ref()
        .ok_or(Error::ConfigurationError(format!(
            "There is no field mapping in the configuration"
        )))?
        .get_field_parser_tree();

    let file_handle = File::open(Path::new(input_file))
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let reader = BufReader::new(file_handle);
    let mut output = Output::new(run_config, plugin_config, metadata, None)?;

    let mut report = RunReport::new();
    let mut record = Record::new();
    for line in reader.lines() {
        match line {
            Ok(line) => {
                let json: Result<JsonValue, serde_json::Error> = serde_json::from_str(&line);
                match json {
                    Ok(json) => parse(
                        json,
                        &mut record,
                        &mut output,
                        &mut parser_tree,
                        &mut report,
                    ),
                    Err(e) => report.add_error(format!("{e}")),
                }
            }
            Err(e) => report.add_error(e.to_string()),
        }
        record.clear();
    }
    report.add_output_report(output.get_report());
    Ok(report)
}

fn parse(
    json: JsonValue,
    record: &mut Record,
    output: &mut Output,
    parser_tree: &mut FieldParserTree,
    report: &mut RunReport,
) {
    let parse_unknown = if let Some(Parser::Ignore()) = parser_tree.default_parser {
        false
    } else if parser_tree.ignore_parsing {
        false
    } else {
        true
    };

    if let JsonValue::Object(map) = json {
        match parse_json_object(map, Some(parser_tree), parse_unknown, record) {
            Ok(()) => {
                if let Err(e) = output.write(record) {
                    report.add_error(e.to_string())
                }
            }
            Err(e) => report.add_error(e.to_string()),
        }
    }
}

pub fn parse_json_object(
    json: Map<String, JsonValue>,
    parser_tree: Option<&FieldParserTree>,
    parse_unknown: bool,
    record: &mut Record,
) -> Result<(), Error> {
    for (name, json_value) in json {
        match parser_tree {
            Some(p_tree) => {
                let value_opt = get_values(&name, json_value, Some(p_tree), parse_unknown)?;
                if let Some(value) = value_opt {
                    p_tree.set_field_value(&name, value, record)?;
                }
            }
            None => {
                if parse_unknown {
                    let value_opt = get_values(&name, json_value, parser_tree, parse_unknown)?;
                    if let Some(value) = value_opt {
                        record.add(&name, value);
                    }
                }
            }
        };
    }

    Ok(())
}

fn get_values(
    input_name: &str,
    json: JsonValue,
    parser_tree: Option<&FieldParserTree>,
    parse_unknown: bool,
) -> Result<Option<Value>, Error> {
    let value = match json {
        JsonValue::Null => Some(Value::Null()),
        JsonValue::String(s) => Some(Value::String(s)),
        JsonValue::Bool(b) => Some(Value::Bool(b)),
        JsonValue::Number(number) => {
            if number.is_f64() {
                Some(Value::Float(number.as_f64().ok_or_else(|| {
                    Error::FieldParserError("f64".into(), input_name.into())
                })?))
            } else if number.is_i64() {
                Some(Value::Int(number.as_i64().ok_or_else(|| {
                    Error::FieldParserError("i64".into(), input_name.into())
                })?))
            } else if number.is_u64() {
                Some(Value::Int(
                    number
                        .as_u64()
                        .ok_or_else(|| Error::FieldParserError("u64".into(), input_name.into()))?
                        as i64,
                ))
            } else {
                None
            }
        }
        JsonValue::Array(values) => {
            let mut val_array = Vec::with_capacity(values.len());
            for json_value in values {
                let value_opt = get_values(input_name, json_value, parser_tree, parse_unknown)?;
                if let Some(value) = value_opt {
                    val_array.push(value);
                }
            }
            Some(Value::Array(val_array))
        }
        JsonValue::Object(map) => match parser_tree {
            Some(parser_tree) => match parser_tree.parsers.get(input_name) {
                Some(parser) => match parser {
                    ParserType::Array { field } => match field.0.as_ref() {
                        ParserType::Object { field_parsers } => {
                            let mut record = Record::with_capacity(map.len());
                            parse_json_object(
                                map,
                                Some(field_parsers),
                                parse_unknown,
                                &mut record,
                            )?;
                            Some(Value::Object(record))
                        }
                        _ => None,
                    },
                    ParserType::Object { field_parsers } => {
                        let mut record = Record::with_capacity(map.len());
                        parse_json_object(map, Some(field_parsers), parse_unknown, &mut record)?;
                        Some(Value::Object(record))
                    }
                    _ => None,
                },
                None => {
                    if parse_unknown {
                        let mut record = Record::with_capacity(map.len());
                        parse_json_object(map, None, parse_unknown, &mut record)?;
                        Some(Value::Object(record))
                    } else {
                        None
                    }
                }
            },
            None => {
                if parse_unknown {
                    let mut record = Record::with_capacity(map.len());
                    parse_json_object(map, None, parse_unknown, &mut record)?;
                    Some(Value::Object(record))
                } else {
                    None
                }
            }
        },
    };
    Ok(value)
}
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs};

    use crate::OutputConfiguration;

    use super::*;
    #[test]
    fn json() {
        let output_folder = ".tmp";
        let base_file_name = "json";
        let targetfile = format!("{output_folder}/{base_file_name}.json_test.jsonl");
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
            false,
            true,
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);
        let xml = fs::read_to_string("test_data/json/json.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let metadata = Metadata::new("test".into());

        let report = parse_json(
            "test_data/json/json_file.json",
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
        assert_eq!(1, lines.len());

        let line = lines[0].as_object().unwrap();
        let int_array = line.get("int_array").unwrap().as_array().unwrap();
        assert_eq!(int_array[1].as_i64().unwrap(), 8);

        let unmaped = line.get("unmapped").unwrap().as_array().unwrap();
        assert_eq!(
            unmaped[1]
                .as_object()
                .unwrap()
                .get("device_updated")
                .unwrap()
                .as_bool()
                .unwrap(),
            true
        );
    }

    #[test]
    fn jsonl() {
        let output_folder = ".tmp";
        let base_file_name = "json";
        let targetfile = format!("{output_folder}/{base_file_name}.web_history.jsonl");
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
            false,
            true,
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);
        let xml = fs::read_to_string("test_data/json/web_history_jsonl.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let metadata = Metadata::new("test".into());

        let report = parse_jsonl(
            "test_data/json/web_history.jsonl",
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
        assert_eq!(3, lines.len());
    }
}
