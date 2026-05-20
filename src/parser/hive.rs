use crate::{
    Error, Metadata, Output, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration,
    windows_utils::{from_filetime, security_descriptor_from_bytes},
};
use dfir_nt_hive::{Hive, KeyNode, KeyValue, KeyValueData, KeyValueDataType, NtHiveError, Result};

use pyo3::prelude::*;
use regex::Regex;
use std::{fs::File, io::Read};
use zerocopy::SplitByteSlice;

const HIVE_KEY_PATH: &str = "path";
const HIVE_KEY_NAME: &str = "name";
const HIVE_KEY_DATE: &str = "mtime";
const HIVE_KEY_SECURITY: &str = "descriptor";
const HIVE_KEY_VALUES: &str = "values";

/// Extract keys from a Windows Hive file
///
/// This function serves as a Python-compatible wrapper for the internal `parse` function.
///
/// # Parameters
/// - `input_file`: Path to the Hive file to be parsed.
/// - `configuration`: `RunConfiguration` specifying parsing settings.
/// - `metadata`: `Metadata` associated with the parsing task.
/// - `root_name`: Optional string to prepend to paths in the output.
/// - `regex`: Optional regex to filter path.
///
#[pyfunction]
pub fn parse_hive_keys(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    root_name: Option<String>,
    regexp: Option<String>,
) -> RunReport {
    match parse_keys(
        input_file,
        run_config,
        plugin_config,
        metadata,
        root_name,
        regexp,
    ) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

/// Extract Keys from a Windows Hive file
///
///  - Applies optional `filter` to restrict processing to a specific subpath.
///
/// # Parameters
/// - `input_file`: Path to the Hive file to be parsed.
/// - `configuration`: `RunConfiguration` specifying parsing settings.
/// - `metadata`: `Metadata` associated with the parsing task.
/// - `root_name`: Optional string to prepend to paths in the output.
/// - `regexp`: Optional regex to filter path.
///
fn parse_keys(
    input_file: &str,
    configuration: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    root_name: Option<String>,
    regexp: Option<String>,
) -> Result<RunReport, Error> {
    let data_mapping = &plugin_config.get_data_type_mapping(None)?;
    let include_values: bool = data_mapping
        .params
        .get("include_values")
        .cloned()
        .unwrap_or("false".to_string())
        .parse()?;

    let truncate_values_after: usize = data_mapping
        .params
        .get("truncate_values_after")
        .cloned()
        .unwrap_or("0".to_string())
        .parse()?;

    let mut hive_file = File::open(input_file)
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let mut data = Vec::with_capacity(hive_file.metadata()?.len() as usize);
    hive_file.read_to_end(&mut data)?;

    let hive = Hive::without_validation(data.as_ref()).map_err(|e| {
        Error::NtHiveError(format!(
            "Error parsing hive file: '{}' -  Error: {e}",
            input_file.to_owned()
        ))
    })?;

    let root_key = hive.root_key_node().map_err(|e| {
        Error::NtHiveError(format!(
            "Error getting root key in file: '{}' -  Error: {e}",
            input_file.to_owned()
        ))
    })?;

    let regex: Option<Regex> = match regexp {
        Some(f) => Some(Regex::new(&f)?),
        None => None,
    };

    let root_name = match root_name {
        Some(name) => name,
        None => "".to_owned(),
    };

    // let mut time_line_builder = TimeLineBuilder::new(
    //     TimeLineType::Standard,
    //     "UTC".to_owned(),
    //     "hive".to_owned(),
    //     "Hive".to_owned(),
    //     1,
    // );
    // time_line_builder.add_related_user_ouput_path(vec![
    //     HIVE_KEY_SECURITY.to_owned(),
    //     HIVE_KEY_SECURITY_OWNER.to_owned(),
    // ]);

    // time_line_builder.add_description_ouput_name(HIVE_KEY_PATH.to_owned());

    let mut key_output = Output::new(configuration.clone(), plugin_config, metadata, None)?;

    let mut report = RunReport::new();
    parse_sub_keys(
        root_key,
        &root_name,
        &mut key_output,
        &regex,
        include_values,
        truncate_values_after,
        &mut report,
    )?;

    report.add_output_report(key_output.get_report());
    Ok(report)
}

/// Recursively processes a Hive key node and its subkeys, writing parsed keys to the output.
///
/// # Parameters
/// - `key_node`: The current `KeyNode` to process.
/// - `path`: The current path in the hive structure (e.g., `\\Subkey\\...`).
/// - `output`: The mutable `Output` instance to write parsed data to.
/// - `regex`: an optional regular expression to filter the output
///
fn parse_sub_keys<B>(
    key_node: KeyNode<B>,
    path: &str,
    key_output: &mut Output,
    regex: &Option<Regex>,
    include_values: bool,
    truncate_values_after: usize,
    report: &mut RunReport,
) -> Result<(), Error>
where
    B: SplitByteSlice,
{
    if let Some(subkeys) = key_node.subkeys() {
        let subkeys = match subkeys {
            Ok(key) => key,
            Err(e) => {
                let error = format!("while parsing sub_keys for '{path}' error: {e}");

                report.add_error(error);
                return Ok(());
            }
        };

        for key_node in subkeys {
            let key_node = key_node?;

            let key_name = key_node.name()?.to_string_lossy();

            let path = format!("{path}\\{key_name}");

            if let Some(reg) = regex
                && !reg.is_match(&path)
            {
                parse_sub_keys(
                    key_node,
                    &path,
                    key_output,
                    regex,
                    include_values,
                    truncate_values_after,
                    report,
                )?;
                continue;
            }

            let filetime = from_filetime(key_node.timestamp());

            let security_descriptor_b = key_node.security_descriptor()?;

            let descriptor = security_descriptor_from_bytes(&security_descriptor_b)?;

            let mut key_record = Record::new();
            key_record.add(HIVE_KEY_NAME, Value::String(key_name.clone()));
            key_record.add(HIVE_KEY_PATH, Value::String(path.clone()));
            key_record.add(HIVE_KEY_DATE, Value::Date(filetime));

            if include_values && let Some(value_iter) = key_node.values() {
                let value_iter = value_iter?;
                let mut values = vec![];
                for value in value_iter {
                    let mut value_record = Record::new();

                    match parse_key_value(value, &mut value_record, truncate_values_after) {
                        Ok(v) => v,
                        Err(e) => {
                            let error = format!("while parsing values for '{path}', error: {e}");

                            report.add_error(error);
                            value_record.add(
                                HIVE_VALUE_ERROR,
                                Value::String(format!("Parsing error: {e}")),
                            );
                        }
                    };
                    values.push(Value::Object(value_record));
                }
                key_record.add(HIVE_KEY_VALUES, Value::Array(values));
            }
            key_record.add(HIVE_KEY_SECURITY, Value::Object(descriptor.to_record()));
            key_output.write(&mut key_record)?;

            parse_sub_keys(
                key_node,
                &path,
                key_output,
                regex,
                include_values,
                truncate_values_after,
                report,
            )?;
        }
    }

    Ok(())
}

/// Extract values from a Windows Hive file
///
/// This function serves as a Python-compatible wrapper for the internal `parse` function.
///
/// # Parameters
/// - `input_file`: Path to the Hive file to be parsed.
/// - `configuration`: `RunConfiguration` specifying parsing settings.
/// - `metadata`: `Metadata` associated with the parsing task.
/// - `root_name`: Optional string to prepend to paths in the output.
/// - `regex`: Optional regex to filter path.
///
#[pyfunction]
pub fn parse_hive_values(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    root_name: Option<String>,
    regexp: Option<String>,
) -> RunReport {
    match parse_values(
        input_file,
        run_config,
        plugin_config,
        metadata,
        root_name,
        regexp,
    ) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

const HIVE_VALUE_NAME: &str = "name"; //VALUE_NAME
const HIVE_VALUE_PATH: &str = "path"; //KEY_PATH
const HIVE_VALUE_DATA: &str = "data";
const HIVE_VALUE_TYPE: &str = "type";
const HIVE_VALUE_SIZE: &str = "size";
const HIVE_VALUE_ERROR: &str = "error";
const HIVE_VALUE_INVALID_SIGNATURE: &str = "invalid_signature";

// fn values_field_mapping() -> Vec<Field> {
//     let qualifiers = Qualifiers::new();

//     vec![
//         Field::Single {
//             name: FieldName::new(
//                 HIVE_VALUE_NAME.to_owned(),
//                 None,
//                 Some(qualifiers.VALUE_NAME.clone()),
//                 None,
//                 None,
//             ),
//             parser: Parser::String(),
//             default_value: None,
//         },
//         Field::Single {
//             name: FieldName::new(
//                 HIVE_VALUE_PATH.to_owned(),
//                 None,
//                 Some(qualifiers.KEY_PATH.clone()),
//                 None,
//                 None,
//             ),
//             parser: Parser::String(),
//             default_value: None,
//         },
//         Field::Single {
//             name: FieldName::new(HIVE_VALUE_TYPE.to_owned(), None, None, None, None),
//             parser: Parser::String(),
//             default_value: None,
//         },
//         Field::Single {
//             name: FieldName::new(HIVE_VALUE_SIZE.to_owned(), None, None, None, None),
//             parser: Parser::Int(),
//             default_value: None,
//         },
//         Field::Single {
//             name: FieldName::new(HIVE_VALUE_DATA.to_owned(), None, None, None, None),
//             parser: Parser::String(),
//             default_value: None,
//         },
//     ]
// }

/// Extract Keys from a Windows Hive file
///
///  - Applies optional `filter` to restrict processing to a specific subpath.
///
/// # Parameters
/// - `input_file`: Path to the Hive file to be parsed.
/// - `configuration`: `RunConfiguration` specifying parsing settings.
/// - `metadata`: `Metadata` associated with the parsing task.
/// - `root_name`: Optional string to prepend to paths in the output.
/// - `regexp`: Optional regex to filter path.
///
fn parse_values(
    input_file: &str,
    configuration: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    root_name: Option<String>,
    regexp: Option<String>,
) -> Result<RunReport, Error> {
    let data_mapping = &plugin_config.data_type_configs[0];

    let truncate_values_after: usize = data_mapping
        .params
        .get("truncate_values_after")
        .cloned()
        .unwrap_or("0".to_string())
        .parse()?;

    let mut hive_file = File::open(input_file)
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let mut data = Vec::with_capacity(hive_file.metadata()?.len() as usize);
    hive_file.read_to_end(&mut data)?;

    let hive = Hive::without_validation(data.as_ref()).map_err(|e| {
        Error::NtHiveError(format!(
            "Error parsing hive file: '{}' -  Error: {e}",
            input_file.to_owned()
        ))
    })?;

    let root_key = hive.root_key_node().map_err(|e| {
        Error::NtHiveError(format!(
            "Error getting root key in file: '{}' -  Error: {e}",
            input_file.to_owned()
        ))
    })?;

    let regex: Option<Regex> = match regexp {
        Some(f) => Some(Regex::new(&f)?),
        None => None,
    };

    let root_name = match root_name {
        Some(name) => name,
        None => "".to_owned(),
    };

    // let key_field_mapping = FieldMapping::new(values_field_mapping(), None);

    let mut key_output = Output::new(configuration.clone(), plugin_config, metadata, None)?;

    let mut report = RunReport::new();
    parse_subvalues(
        root_key,
        &root_name,
        &mut key_output,
        &regex,
        truncate_values_after,
        &mut report,
    )?;

    report.add_output_report(key_output.get_report());
    Ok(report)
}

/// Recursively processes a Hive key node and its subkeys, writing parsed values to the output.
///
/// # Parameters
/// - `key_node`: The current `KeyNode` to process.
/// - `path`: The current path in the hive structure (e.g., `\\Subkey\\...`).
/// - `output`: The mutable `Output` instance to write parsed data to.
/// - `regex`: an optional regular expression to filter the output
///
fn parse_subvalues<B>(
    key_node: KeyNode<B>,
    path: &str,
    output: &mut Output,
    regex: &Option<Regex>,
    truncate_after: usize,
    report: &mut RunReport,
) -> Result<(), Error>
where
    B: SplitByteSlice,
{
    if let Some(subkeys) = key_node.subkeys() {
        let subkeys = match subkeys {
            Ok(key) => key,
            Err(e) => {
                report.add_error(format!("while parsing sub_keys for '{path}' error: {e}"));
                return Ok(());
            }
        };

        for key_node in subkeys {
            let key_node = key_node?;

            let key_name = key_node.name()?.to_string_lossy();

            let path = format!("{path}\\{key_name}");

            if let Some(reg) = regex
                && !reg.is_match(&path)
            {
                parse_subvalues(key_node, &path, output, regex, truncate_after, report)?;
                continue;
            }

            let mut record = Record::new();
            record.add(HIVE_VALUE_PATH, Value::String(path.clone()));

            if let Some(value_iter) = key_node.values() {
                let value_iter = value_iter?;

                for value in value_iter {
                    let mut value_record = record.clone();

                    match parse_key_value(value, &mut value_record, truncate_after) {
                        Ok(v) => v,
                        Err(e) => {
                            report.add_error(format!(
                                "while parsing values for '{path}', error: {e}"
                            ));
                            value_record.add(
                                HIVE_VALUE_ERROR,
                                Value::String(format!("Parsing error: {e}")),
                            );
                        }
                    };

                    output.write(&mut value_record)?;
                }
            }

            parse_subvalues(key_node, &path, output, regex, truncate_after, report)?;
        }
    }

    Ok(())
}

///
/// fill the value record
///
fn parse_key_value<B>(
    value: Result<KeyValue<'_, B>, NtHiveError>,
    record: &mut Record,
    truncate_after: usize,
) -> Result<(), Error>
where
    B: SplitByteSlice,
{
    let value: KeyValue<'_, B> = value?;

    let mut value_name = value.name()?.to_string_lossy();

    if value_name.is_empty() {
        value_name.push_str("Default");
    }
    record.add(HIVE_VALUE_NAME, Value::String(value_name));

    match value.data_type() {
        Ok(value_type) => match value_type {
            KeyValueDataType::RegSZ => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_SZ".to_owned()));

                let string_data = value.string_data()?;
                let value = string_value(string_data, truncate_after);
                record.add(HIVE_VALUE_DATA, value);
            }

            KeyValueDataType::RegExpandSZ => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_EXPAND_SZ".to_owned()));

                let string_data = value.string_data()?;
                let value = string_value(string_data, truncate_after);

                record.add(HIVE_VALUE_DATA, value);
            }
            KeyValueDataType::RegBinary => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_BINARY".to_owned()));
                let hex_val = get_hex_value(&value, truncate_after)?;
                record.add(HIVE_VALUE_DATA, hex_val);
            }
            KeyValueDataType::RegDWord => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_DWORD_LE".to_owned()));
                let dword_data = value.dword_data()?;
                let value = Value::Int(dword_data as i64);
                record.add(HIVE_VALUE_DATA, value);
            }

            KeyValueDataType::RegDWordBigEndian => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_DWORD_BE".to_owned()));
                let dword_data = value.dword_data()?;
                let value = Value::Int(dword_data as i64);
                record.add(HIVE_VALUE_DATA, value);
            }
            KeyValueDataType::RegMultiSZ => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_MULTI_SZ".to_owned()));
                let multi_string_data = value.multi_string_data()?.collect::<Result<Vec<_>>>()?;
                let field: Vec<Value> = multi_string_data
                    .iter()
                    .map(|v| Value::String(v.to_string()))
                    .collect();
                record.add(HIVE_VALUE_DATA, Value::Array(field));
            }
            KeyValueDataType::RegQWord => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_QWORD_LE".to_owned()));

                let qword_data = value.qword_data()?;
                let value = string_value(qword_data.to_string(), truncate_after);
                record.add(HIVE_VALUE_DATA, value);
            }
            KeyValueDataType::RegNone => {
                record.add(
                    HIVE_VALUE_TYPE,
                    Value::String("REG_FIRST_INVALID".to_owned()),
                );
                let hex_val = get_hex_value(&value, truncate_after)?;
                record.add(HIVE_VALUE_DATA, hex_val);
            }
            KeyValueDataType::RegLink => {
                record.add(HIVE_VALUE_TYPE, Value::String("REG_LINK".to_owned()));
                let hex_val = get_hex_value(&value, truncate_after)?;
                record.add(HIVE_VALUE_DATA, hex_val);
            }
            KeyValueDataType::RegResourceList => {
                record.add(
                    HIVE_VALUE_TYPE,
                    Value::String("REG_RESOURCE_LIST".to_owned()),
                );

                let hex_val = get_hex_value(&value, truncate_after)?;
                record.add(HIVE_VALUE_DATA, hex_val);
            }
            KeyValueDataType::RegFullResourceDescriptor => {
                record.add(
                    HIVE_VALUE_TYPE,
                    Value::String("REG_FULL_RESOURCE_DESCRIPTOR".to_owned()),
                );

                let hex_val = get_hex_value(&value, truncate_after)?;
                record.add(HIVE_VALUE_DATA, hex_val);
            }
            KeyValueDataType::RegResourceRequirementsList => {
                record.add(
                    HIVE_VALUE_TYPE,
                    Value::String("REG_RESOURCE_REQUIREMENT_LIST".to_owned()),
                );

                let hex_val = get_hex_value(&value, truncate_after)?;
                record.add(HIVE_VALUE_DATA, hex_val);
            }
        },

        Err(_) => {
            let code = value.data_type_code();
            record.add(HIVE_VALUE_TYPE, Value::String(format!("{code}(unknown)")));
            let hex_val = get_hex_value(&value, truncate_after)?;
            record.add(HIVE_VALUE_DATA, hex_val);
        }
    };

    record.add(HIVE_VALUE_SIZE, Value::Int(value.data_size() as i64));
    if !value.validate_signature()? {
        record.add(HIVE_VALUE_INVALID_SIGNATURE, Value::Bool(true));
    }

    Ok(())
}
fn string_value(value: String, truncate_after: usize) -> Value {
    if truncate_after > 0 && value.len() > truncate_after {
        let sub: String = value.chars().take(truncate_after).collect();
        Value::String(format!("{}[..]", (&sub)))
    } else {
        Value::String(value)
    }
}

fn get_hex_value<B>(value: &KeyValue<'_, B>, truncate_after: usize) -> Result<Value, Error>
where
    B: SplitByteSlice,
{
    let binary_data = value.data()?;
    Ok(match binary_data {
        KeyValueData::Small(data) => {
            let hex_str = if truncate_after > 0 && data.len() > truncate_after {
                format!("0x{}[..]", hex::encode(&data[0..truncate_after]))
            } else {
                format!("0x{}", hex::encode(data))
            };
            Value::String(hex_str)
        }
        KeyValueData::Big(iter) => {
            let data_vec = iter.collect::<Result<Vec<_>>>()?;

            let data_size = value.data_size();
            let mut data: Vec<u8> = Vec::with_capacity(data_size as usize);
            for slice in data_vec {
                data.extend_from_slice(slice);
            }

            let hex_str = if truncate_after > 0 && data.len() > truncate_after {
                format!("0x{}[..]", hex::encode(&data[0..truncate_after]))
            } else {
                format!("0x{}", hex::encode(data))
            };

            Value::String(hex_str)
        }
    })
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, fs, path::Path};

    use crate::OutputConfiguration;

    use super::*;
    #[test]
    fn test_parse_hive_keys() {
        let output_folder = ".tmp";
        let base_file_name = "hive_test";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            true,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_keys(
            "test_data/hive/testhive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            None,
        );
        assert_eq!(report.last_error, None);

        let result = &report.output_reports[0];

        let expected_lines = 527;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
    }

    #[test]
    fn test_parse_hive_key_values() {
        let output_folder = ".tmp";
        let base_file_name = "hive_key_values";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_keys(
            "test_data/hive/testhive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            None,
        );
        assert_eq!(report.last_error, None);

        let result = &report.output_reports[0];

        let expected_lines = 527;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());

        let data = lines[0].as_object().unwrap();

        let path = data.get("values").unwrap().as_array().unwrap();
        assert_eq!(path.len(), 3);
        let key_value = path[0].as_object().unwrap();
        let name = key_value.get("name").unwrap().as_str().unwrap();
        assert_eq!("A", name);

        let kv_type = key_value.get("type").unwrap().as_str().unwrap();
        assert_eq!("REG_BINARY", kv_type);

        let kv_data = key_value.get("data").unwrap().as_str().unwrap();
        assert_eq!(
            "0x4141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141414141[..]",
            kv_data
        );

        let size = key_value.get("size").unwrap().as_i64().unwrap();
        assert_eq!(16343, size);
        // "type":"REG_BINARY","data":"0x4141414141414141414141414141414141414141414141414141414141414141[..]","size":16343

        // assert_eq!(path, "\\SAM\\Domains\\Account\\Users\\Names\\Guest");
    }

    #[test]
    fn test_filter_keys() {
        let output_folder = ".tmp";
        let base_file_name = "hive_sam_filter";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            true,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_keys(
            "test_data/hive/SAM.hive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            Some("\\\\SAM\\\\Domains\\\\Account\\\\Users\\\\Names".to_owned()),
        );
        assert_eq!(report.last_error, None);

        let result = &report.output_reports[0];

        let expected_lines = 6;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
        let data = lines[4].as_object().unwrap();

        let path = data.get("path").unwrap().as_str().unwrap();
        assert_eq!(path, "\\SAM\\Domains\\Account\\Users\\Names\\Guest");
    }

    #[test]
    fn test_bad_filter() {
        let output_folder = ".tmp";
        let base_file_name = "hive_sam_bad_filter";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            true,
            true,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_keys(
            "test_data/hive/SAM.hive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            Some("\\error".to_owned()),
        );
        assert_eq!(
            report.last_error,
            Some(
                "regex parse error:\n    \\error\n    ^^\nerror: unrecognized escape sequence"
                    .to_owned()
            )
        );
    }

    #[test]
    fn test_root_name_filter() {
        let output_folder = ".tmp";
        let base_file_name = "hive_root_name_filter";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            true,
            true,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_keys(
            "test_data/hive/testhive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            Some("HELLO".to_owned()),
            Some("HELLO\\\\subpath-test".to_owned()),
        );
        assert_eq!(report.last_error, None);

        let result = &report.output_reports[0];

        let expected_lines = 7;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
        let data = lines[2].as_object().unwrap();

        let key_name = data.get("name:key_name").unwrap().as_str().unwrap();
        assert_eq!(key_name, "with-single-level-subkey");
    }

    #[test]
    fn test_parse_hive_values() {
        let output_folder = ".tmp";
        let base_file_name = "hive_values_test";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_values(
            "test_data/hive/testhive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            None,
        );
        assert_eq!(report.last_error, None);

        let result = &report.output_reports[0];

        let expected_lines = 12;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());

        let data = lines[4].as_object().unwrap();

        let parsed = data.get("data").unwrap().as_str().unwrap();
        assert_eq!(parsed, "sz-test");
    }

    #[test]
    fn test_parse_sam_values() {
        let output_folder = ".tmp";
        let base_file_name = "hive_values_sam_test";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_values(
            "test_data/hive/SAM.hive",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            None,
        );
        assert_eq!(report.last_error, None);

        let result = &report.output_reports[0];

        let expected_lines = 79;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());

        let data = lines[11].as_object().unwrap();

        let parsed = data.get("type").unwrap().as_str().unwrap();
        assert_eq!(parsed, "513(unknown)");
    }

    #[test]
    fn test_parse_nt_user_values() {
        let output_folder = ".tmp";
        let base_file_name = "hive_values_nt_user_values";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_values(
            "test_data/hive/NTUSER.dat",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            None,
        );
        assert_eq!(
            report.last_error,
            Some("while parsing values for '\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.controlcenter.uistate\\windows.data.controlcenter.uistate', error: Invalid offset:'-4'".to_owned())
        );

        let result = &report.output_reports[0];

        let expected_lines = 4484;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
    }

    #[test]
    fn test_parse_nt_user_key_values() {
        let output_folder = ".tmp";
        let base_file_name = "hive_nt_user_key_values";
        let targetfile = format!("{output_folder}/{base_file_name}.hive.jsonl");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/hive/hive.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_hive_keys(
            "test_data/hive/NTUSER.dat",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            None,
            None,
        );
        assert_eq!(
            report.last_error,
            Some("while parsing values for '\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.controlcenter.uistate\\windows.data.controlcenter.uistate', error: Invalid offset:'-4'".to_owned())
        );

        let result = &report.output_reports[0];

        let expected_lines = 2946;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());
    }
}
