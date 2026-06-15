use crate::{
    Error, Metadata, Output, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration, field_mapping::FieldParserTree,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as enc64};

use pyo3::prelude::*;
use rusqlite::Connection;

/// Parses an SQLite database file using the provided query and configuration.
///
/// This function serves as a Python-compatible wrapper for the internal `parse` function.
/// It handles any errors by setting them on the returned `RunReport`.
///
/// # Arguments
/// * `input_file` - Path to the SQLite database file
/// * `configuration` - Run configuration parameters
/// * `metadata` - Metadata associated with the run
/// * `query` - SQL query to execute
///
#[pyfunction]
pub fn parse_sqlite(
    input_file: &str,
    configuration: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: usize,
) -> RunReport {
    match parse(
        input_file,
        configuration,
        plugin_config,
        metadata,
        log_before_fail,
    ) {
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
    configuration: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: usize,
) -> Result<RunReport, Error> {
    let conn = Connection::open(input_file)?;
    let data_mapping = &plugin_config.get_data_type_mapping(None)?;
    let field_mapping = data_mapping
        .field_mapping
        .clone()
        .ok_or(Error::CannotBeEmpty("FieldMapping".to_owned()))?;

    let query = data_mapping
        .params
        .get("query")
        .cloned()
        .ok_or(Error::ConfigurationError(
            "'query' is not set in the configuration ".to_string(),
        ))?;

    let mut field_parsers = field_mapping.get_field_parser_tree();

    let mut stmt = conn.prepare(&query)?;

    let mut output = Output::new(configuration, plugin_config, metadata, None)?;

    let column_names: Vec<String> = {
        stmt.column_names()
            .iter()
            .map(|f| f.to_owned().to_owned())
            .collect()
    };
    let mut report = RunReport::new();
    let mut record = Record::new();
    let mut rows = stmt.query([])?;
    let mut num_row = 0;

    while let Some(row) = rows.next()? {
        if let Err(err) = process_row(
            &mut output,
            &column_names,
            &mut record,
            row,
            &mut field_parsers,
        ) {
            record.clear();

            report.add_error(format!("Line {num_row}, error: {err}"));
            if report.num_errors >= log_before_fail {
                break;
            }
        }
        num_row += 1;
    }

    report.add_output_report(output.get_report());
    Ok(report)
}

fn process_row(
    output: &mut Output,
    column_names: &[String],
    record: &mut Record,
    row: &rusqlite::Row<'_>,
    field_parsers: &mut FieldParserTree,
) -> Result<(), Error> {
    for (pos, name) in column_names.iter().enumerate() {
        let mut parser_opt = field_parsers.get_parser(name);

        match row.get_ref(pos)? {
            rusqlite::types::ValueRef::Null => match &mut parser_opt {
                Some(parser) => parser.set_value(Value::Null(), record)?,
                None => record.add(name, Value::Null()),
            },
            rusqlite::types::ValueRef::Integer(v) => match &mut parser_opt {
                Some(parser) => parser.parse(Some(&v.to_string()), record)?,
                None => record.add(name, Value::Int(v)),
            },
            rusqlite::types::ValueRef::Real(v) => match &mut parser_opt {
                Some(parser) => parser.parse(Some(&v.to_string()), record)?,
                None => record.add(name, Value::Float(v)),
            },
            rusqlite::types::ValueRef::Text(items) => {
                let converted = String::from_utf8_lossy(items).to_string();
                match &mut parser_opt {
                    Some(parser) => parser.parse(Some(&converted), record)?,
                    None => record.add(name, Value::String(converted)),
                }
            }
            rusqlite::types::ValueRef::Blob(items) => {
                let converted = enc64.encode(items);
                match &mut parser_opt {
                    Some(parser) => parser.parse(Some(&converted), record)?,
                    None => record.add(name, Value::String(converted)),
                }
            }
        };
    }
    output.write(record)
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, fs};

    use crate::OutputConfiguration;

    use super::*;

    #[test]
    fn test_sqlite() {
        let output_folder = ".tmp";
        let base_file_name = "sqlite_test";
        let targetfile = format!("{output_folder}/{base_file_name}.sqlite.jsonl");

        let paths = fs::read_dir(output_folder).unwrap();

        for path in paths {
            let path_str: String = path.unwrap().path().display().to_string();
            if path_str.starts_with(&targetfile) {
                fs::remove_file(&path_str).unwrap();
            }
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            true,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/sqlite/sqlite.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_sqlite(
            "test_data/sqlite/sqlite.db",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            0,
        );
        assert_eq!(None, report.last_error);

        let result = &report.output_reports[0];

        let expected_lines = 2;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
        let data = lines[1].as_object().unwrap();

        let path = data.get("name").unwrap().as_str().unwrap();
        assert_eq!(path, "planet");
    }

    #[test]
    fn test_sqlite_timeline() {
        let output_folder = ".tmp";
        let base_file_name = "sqlite_activity_test";
        let targetfile = format!("{output_folder}/{base_file_name}.activity_cache.jsonl");

        let paths = fs::read_dir(output_folder).unwrap();

        for path in paths {
            let path_str: String = path.unwrap().path().display().to_string();
            if path_str.starts_with(&targetfile) {
                fs::remove_file(&path_str).unwrap();
            }
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/sqlite/activities_cache.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_sqlite(
            "test_data/sqlite/activities_cache.db",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            0,
        );
        assert_eq!(None, report.last_error);

        let result = &report.output_reports[0];

        let expected_lines = 51;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
        let data = lines[3].as_object().unwrap();

        let additional_description = data
            .get("additional_description")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(
            additional_description,
            "app_activity_id: default$windows.data.input.devices.pensyncedsettings|windows.data.input.devices.pensyncedsettings - payload: UTBJQkFBSUJBQT09"
        );
    }
}
