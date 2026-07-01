use std::io::{BufRead, BufReader, Read};
use std::{fs::File, path::Path};

use crate::field_mapping::ParserType;
use crate::{
    Error, FieldParser, FieldParserTree, Metadata, Output, Parser, Record, RunConfiguration,
    RunReport, Value, configuration::PluginConfiguration, field::MultiInputContext, parser::runner,
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
        .ok_or(Error::ConfigurationError(
            "There is no field mapping in the configuration".to_string(),
        ))?
        .get_field_parser_tree();
    let mut output = Output::new(run_config, plugin_config, metadata, None)?;
    let mut multi_input_context = MultiInputContext::new(parser_tree.multi_input_group_count);

    parse(
        json,
        &mut Record::new(),
        &mut output,
        &mut parser_tree,
        &mut multi_input_context,
        &mut report,
    );

    runner::finalize_output(&mut output, &mut report);
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
        .ok_or(Error::ConfigurationError(
            "There is no field mapping in the configuration".to_string(),
        ))?
        .get_field_parser_tree();

    let file_handle = File::open(Path::new(input_file))
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let reader = BufReader::new(file_handle);
    let mut output = Output::new(run_config, plugin_config, metadata, None)?;

    let mut report = RunReport::new();
    let mut record = Record::new();
    let mut multi_input_context = MultiInputContext::new(parser_tree.multi_input_group_count);
    for line in reader.lines() {
        multi_input_context.clear();
        match line {
            Ok(line) => {
                let json: Result<JsonValue, serde_json::Error> = serde_json::from_str(&line);
                match json {
                    Ok(json) => parse(
                        json,
                        &mut record,
                        &mut output,
                        &mut parser_tree,
                        &mut multi_input_context,
                        &mut report,
                    ),
                    Err(e) => report.add_error(format!("{e}")),
                }
            }
            Err(e) => report.add_error(e.to_string()),
        }
        record.clear();
    }
    runner::finalize_output(&mut output, &mut report);
    Ok(report)
}

fn parse(
    json: JsonValue,
    record: &mut Record,
    output: &mut Output,
    parser_tree: &mut FieldParserTree,
    multi_input_context: &mut MultiInputContext,
    report: &mut RunReport,
) {
    let parse_unknown = if let Some(Parser::Ignore()) = parser_tree.default_parser {
        false
    } else {
        !parser_tree.ignore_parsing
    };

    if let JsonValue::Object(map) = json {
        match parse_json_object(
            map,
            Some(parser_tree),
            parse_unknown,
            record,
            multi_input_context,
        ) {
            Ok(()) => {
                if let Err(e) = output.write(record) {
                    report.add_error(e.to_string())
                }
            }
            Err(e) => report.add_error(e.to_string()),
        }
    }
}

fn parse_mapped_string_field(
    input_name: &str,
    value: &str,
    parser_tree: &FieldParserTree,
    record: &mut Record,
    multi_input_context: &mut MultiInputContext,
) -> Result<bool, Error> {
    match parser_tree.parsers.get(input_name) {
        Some(ParserType::Field { parser }) => {
            parser.parse_with_context(Some(value), record, multi_input_context)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn mapped_array_field_parser<'a>(
    input_name: &str,
    parser_tree: Option<&'a FieldParserTree>,
) -> Option<&'a FieldParser> {
    match parser_tree.and_then(|tree| tree.parsers.get(input_name)) {
        Some(ParserType::Array { field }) => match field.0.as_ref() {
            ParserType::Field { parser } => Some(parser),
            _ => None,
        },
        _ => None,
    }
}

pub fn parse_json_object(
    json: Map<String, JsonValue>,
    parser_tree: Option<&FieldParserTree>,
    parse_unknown: bool,
    record: &mut Record,
    multi_input_context: &mut MultiInputContext,
) -> Result<(), Error> {
    for (name, json_value) in json {
        match parser_tree {
            Some(p_tree) => {
                if !parse_unknown && !p_tree.parsers.contains_key(&name) {
                    continue;
                }

                if let JsonValue::String(value) = &json_value
                    && parse_mapped_string_field(&name, value, p_tree, record, multi_input_context)?
                {
                    continue;
                }

                let value_opt = get_values(
                    &name,
                    json_value,
                    Some(p_tree),
                    parse_unknown,
                    multi_input_context,
                )?;
                if let Some(value) = value_opt {
                    p_tree.set_field_value(&name, value, record)?;
                }
            }
            None => {
                if parse_unknown {
                    let value_opt = get_values(
                        &name,
                        json_value,
                        parser_tree,
                        parse_unknown,
                        multi_input_context,
                    )?;
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
    multi_input_context: &mut MultiInputContext,
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
            let array_parser = mapped_array_field_parser(input_name, parser_tree);
            let mut val_array = Vec::with_capacity(values.len());
            for json_value in values {
                let value_opt = match (json_value, array_parser) {
                    (JsonValue::String(value), Some(parser)) => {
                        parser.parse_into_value(Some(&value))?
                    }
                    (json_value, _) => get_values(
                        input_name,
                        json_value,
                        parser_tree,
                        parse_unknown,
                        multi_input_context,
                    )?,
                };
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
                            let mut element_context =
                                MultiInputContext::new(field_parsers.multi_input_group_count);
                            parse_json_object(
                                map,
                                Some(field_parsers),
                                parse_unknown,
                                &mut record,
                                &mut element_context,
                            )?;
                            Some(Value::Object(record))
                        }
                        _ => None,
                    },
                    ParserType::Object { field_parsers } => {
                        let mut record = Record::with_capacity(map.len());
                        parse_json_object(
                            map,
                            Some(field_parsers),
                            parse_unknown,
                            &mut record,
                            multi_input_context,
                        )?;
                        Some(Value::Object(record))
                    }
                    _ => None,
                },
                None => {
                    if parse_unknown {
                        let mut record = Record::with_capacity(map.len());
                        parse_json_object(
                            map,
                            None,
                            parse_unknown,
                            &mut record,
                            multi_input_context,
                        )?;
                        Some(Value::Object(record))
                    } else {
                        None
                    }
                }
            },
            None => {
                if parse_unknown {
                    let mut record = Record::with_capacity(map.len());
                    parse_json_object(map, None, parse_unknown, &mut record, multi_input_context)?;
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
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

    use crate::{
        DateInputCodec, Field, FieldMapping, FieldName, OutputConfiguration, Parser,
        configuration::DataTypeMapping,
        field::{ArrayField, MultiInputField, MultiParser},
    };

    use super::*;

    fn remove_dir_if_exists(path: &Path) {
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }

    fn test_folder(name: &str) -> PathBuf {
        let path = Path::new(".tmp").join(name);
        remove_dir_if_exists(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn output_conf(base_file_name: &str, output_folder: &Path) -> OutputConfiguration {
        OutputConfiguration::new(
            base_file_name.to_string(),
            output_folder.display().to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            true,
            HashMap::new(),
        )
    }

    fn json_plugin_config(data_type: &str, field_mapping: FieldMapping) -> PluginConfiguration {
        PluginConfiguration {
            plugin: "json-test".to_string(),
            file_encoding: "UTF_8".to_string(),
            data_type_configs: vec![DataTypeMapping {
                data_type: data_type.to_string(),
                description: None,
                default_date_pattern: DateInputCodec::Iso(),
                params: HashMap::new(),
                timeline: None,
                field_mapping: Some(field_mapping),
                has_primary_key: false,
            }],
        }
    }

    fn simple_event_mapping() -> FieldMapping {
        FieldMapping::new(
            vec![Field::Single {
                name: FieldName::new("event".to_owned(), false, None, None, None),
                parser: Parser::String(),
                default_value: None,
            }],
            None,
        )
    }

    fn nested_json_mapping() -> FieldMapping {
        FieldMapping::new(
            vec![
                Field::Single {
                    name: FieldName::new("event".to_owned(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
                Field::Object {
                    name: FieldName::new("details".to_owned(), false, None, None, None),
                    ignore: false,
                    fields: vec![
                        Field::Single {
                            name: FieldName::new("user".to_owned(), false, None, None, None),
                            parser: Parser::String(),
                            default_value: None,
                        },
                        Field::Single {
                            name: FieldName::new("success".to_owned(), false, None, None, None),
                            parser: Parser::Bool(),
                            default_value: None,
                        },
                    ],
                },
                Field::Array(ArrayField::new(Field::Object {
                    name: FieldName::new("items".to_owned(), false, None, None, None),
                    ignore: false,
                    fields: vec![
                        Field::Single {
                            name: FieldName::new("name".to_owned(), false, None, None, None),
                            parser: Parser::String(),
                            default_value: None,
                        },
                        Field::Single {
                            name: FieldName::new("size".to_owned(), false, None, None, None),
                            parser: Parser::Int(),
                            default_value: None,
                        },
                    ],
                })),
            ],
            None,
        )
    }

    fn multi_input_mapping() -> FieldMapping {
        FieldMapping::new(
            vec![Field::Multi(MultiInputField::new(
                vec![
                    Field::Single {
                        name: FieldName::new("left".to_owned(), false, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    },
                    Field::Single {
                        name: FieldName::new("right".to_owned(), false, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    },
                ],
                FieldName::new(
                    "joined".to_owned(),
                    false,
                    Some("joined".to_owned()),
                    None,
                    None,
                ),
                MultiParser::Join("-".to_owned(), false),
            ))],
            None,
        )
    }

    #[test]
    fn jsonl_multi_input_partials_do_not_leak_between_records() {
        let output_folder = test_folder("jsonl_multi_input_record_scoped");
        let input_path = output_folder.join("input.jsonl");
        fs::write(
            &input_path,
            "{\"left\":\"stale-left\"}\n{\"right\":\"fresh-right\"}\n{\"left\":\"good-left\",\"right\":\"good-right\"}\n",
        )
        .unwrap();
        let run_config =
            RunConfiguration::new(vec![output_conf("multi", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_multi", multi_input_mapping());

        let report = parse_jsonl(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("host-json".into()),
        );

        assert_eq!(report.num_errors, 0);
        let output_path = output_folder.join("multi.json_multi.jsonl");
        let output = fs::read_to_string(output_path).unwrap();
        let records: Vec<serde_json::Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(records.len(), 3);
        assert!(records[0]["joined"].is_null());
        assert!(records[1]["joined"].is_null());
        assert_eq!(records[2]["joined"], "good-left-good-right");
    }

    #[test]
    fn jsonl_malformed_line_records_error_and_continues() {
        let output_folder = test_folder("jsonl_malformed_line");
        let input_path = output_folder.join("input.jsonl");
        fs::write(
            &input_path,
            "{\"event\":\"first\"}\n{not valid json}\n{\"event\":\"second\"}\n",
        )
        .unwrap();
        let run_config =
            RunConfiguration::new(vec![output_conf("malformed", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_edge", simple_event_mapping());

        let report = parse_jsonl(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.num_errors, 1);
        assert!(report.last_error.is_some());
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 2);

        let jsonl = fs::read_to_string(output_folder.join("malformed.json_edge.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0]["event"], "first");
        assert_eq!(lines[1]["event"], "second");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn jsonl_empty_input_produces_empty_output_report() {
        let output_folder = test_folder("jsonl_empty_input");
        let input_path = output_folder.join("empty.jsonl");
        fs::write(&input_path, "").unwrap();
        let run_config =
            RunConfiguration::new(vec![output_conf("empty", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_edge", simple_event_mapping());

        let report = parse_jsonl(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.num_errors, 0);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 0);
        let jsonl = fs::read_to_string(output_folder.join("empty.json_edge.jsonl")).unwrap();
        assert!(jsonl.is_empty());

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn json_nested_objects_and_arrays_follow_mapping() {
        let output_folder = test_folder("json_nested_objects_arrays");
        let input_path = output_folder.join("nested.json");
        fs::write(
            &input_path,
            r#"{
                "event": "login",
                "details": { "user": "alice", "success": true },
                "items": [
                    { "name": "one", "size": 1 },
                    { "name": "two", "size": 2 }
                ]
            }"#,
        )
        .unwrap();
        let run_config =
            RunConfiguration::new(vec![output_conf("nested", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_edge", nested_json_mapping());

        let report = parse_json(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 1);
        let jsonl = fs::read_to_string(output_folder.join("nested.json_edge.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(jsonl.trim()).unwrap();
        assert_eq!(line["event"], "login");
        assert_eq!(line["details"]["user"], "alice");
        assert_eq!(line["details"]["success"], true);
        assert_eq!(line["items"][0]["name"], "one");
        assert_eq!(line["items"][1]["size"], 2);

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn json_top_level_non_object_writes_no_records() {
        let output_folder = test_folder("json_top_level_non_object");
        let input_path = output_folder.join("array.json");
        fs::write(&input_path, r#"[{"event":"ignored"}]"#).unwrap();
        let run_config =
            RunConfiguration::new(vec![output_conf("array", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_edge", simple_event_mapping());

        let report = parse_json(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 0);
        let jsonl = fs::read_to_string(output_folder.join("array.json_edge.jsonl")).unwrap();
        assert!(jsonl.is_empty());

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn json_default_ignore_suppresses_unknown_nested_objects() {
        let output_folder = test_folder("json_default_ignore_nested");
        let input_path = output_folder.join("nested_unknown.json");
        fs::write(
            &input_path,
            r#"{
                "details": { "secret": "hidden" }
            }"#,
        )
        .unwrap();
        let run_config =
            RunConfiguration::new(vec![output_conf("ignored", &output_folder)], true, None);
        let plugin_config = json_plugin_config(
            "json_edge",
            FieldMapping::new(vec![], Some(Parser::Ignore())),
        );

        let report = parse_json(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 1);
        let jsonl = fs::read_to_string(output_folder.join("ignored.json_edge.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(jsonl.trim()).unwrap();
        assert!(line.get("details").is_none());
        assert_eq!(line["ogre_md"]["computer"], "test");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn json_default_ignore_only_emits_mapped_primitives() {
        let output_folder = test_folder("json_default_ignore_primitives");
        let input_path = output_folder.join("unknown_primitives.json");
        fs::write(
            &input_path,
            r#"{
                "known": "visible",
                "unknown_string": "hidden",
                "unknown_int": 7,
                "unknown_bool": true,
                "unknown_null": null,
                "unknown_array": ["hidden", 1, false]
            }"#,
        )
        .unwrap();
        let field_mapping = FieldMapping::new(
            vec![Field::Single {
                name: FieldName::new("known".to_owned(), false, None, None, None),
                parser: Parser::String(),
                default_value: None,
            }],
            Some(Parser::Ignore()),
        );
        let run_config =
            RunConfiguration::new(vec![output_conf("ignored", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_edge", field_mapping);

        let report = parse_json(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 1);
        let jsonl = fs::read_to_string(output_folder.join("ignored.json_edge.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(jsonl.trim()).unwrap();
        assert_eq!(line["known"], "visible");
        assert!(line.get("unknown_string").is_none());
        assert!(line.get("unknown_int").is_none());
        assert!(line.get("unknown_bool").is_none());
        assert!(line.get("unknown_null").is_none());
        assert!(line.get("unknown_array").is_none());
        assert_eq!(line["ogre_md"]["computer"], "test");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn json_string_primitives_apply_configured_parsers_without_coercing_native_values() {
        let output_folder = test_folder("json_string_primitives_configured_parsers");
        let input_path = output_folder.join("input.json");
        fs::write(
            &input_path,
            r#"{
                "string_int": "42",
                "string_bool": "false",
                "native_int": 7,
                "native_bool": true
            }"#,
        )
        .unwrap();

        let field_mapping = FieldMapping::new(
            vec![
                Field::Single {
                    name: FieldName::new(
                        "string_int".to_owned(),
                        false,
                        Some("parsed_int".to_owned()),
                        None,
                        None,
                    ),
                    parser: Parser::Int(),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new(
                        "string_bool".to_owned(),
                        false,
                        Some("parsed_bool".to_owned()),
                        None,
                        None,
                    ),
                    parser: Parser::Bool(),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("native_int".to_owned(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("native_bool".to_owned(), false, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
            None,
        );
        let run_config =
            RunConfiguration::new(vec![output_conf("primitive", &output_folder)], true, None);
        let plugin_config = json_plugin_config("json_edge", field_mapping);

        let report = parse_json(
            input_path.to_str().unwrap(),
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 1);
        let jsonl = fs::read_to_string(output_folder.join("primitive.json_edge.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(jsonl.trim()).unwrap();
        assert_eq!(line["parsed_int"], 42);
        assert_eq!(line["parsed_bool"], false);
        assert_eq!(line["native_int"], 7);
        assert_eq!(line["native_bool"], true);

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn json_date_string_mapping_emits_timeline_record() {
        let output_folder = test_folder("json_date_string_timeline");
        let output_conf = OutputConfiguration::new(
            "json_timeline".to_string(),
            output_folder.display().to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            HashMap::new(),
        );
        let run_config = RunConfiguration::new(vec![output_conf], true, None);
        let xml = fs::read_to_string("test_data/json/json.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_json(
            "test_data/json/json_file.json",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 1);
        let jsonl =
            fs::read_to_string(output_folder.join("json_timeline.json_test.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 1);
        let line = lines[0].as_object().unwrap();
        assert_eq!(
            line.get("timestamp").unwrap().as_str().unwrap(),
            "2016-04-26T21:30:13.520562+00:00"
        );
        assert_eq!(
            line.get("timestamp_meaning").unwrap().as_str().unwrap(),
            "System Time"
        );
        assert_eq!(
            line.get("data").unwrap()["system"]["time_created"]["system_time"],
            "2016-04-26T21:30:13.520562+00:00"
        );

        remove_dir_if_exists(&output_folder);
    }

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
        assert!(
            unmaped[1]
                .as_object()
                .unwrap()
                .get("device_updated")
                .unwrap()
                .as_bool()
                .unwrap()
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
