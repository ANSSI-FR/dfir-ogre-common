use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
};

use pyo3::prelude::*;

use crate::{
    Error, Field, FieldParser, Metadata, Output, Parser, Record, RunConfiguration, RunReport,
    Value,
    configuration::{PluginConfiguration, encoding_reader_builder},
    field_mapping::ParserType,
};
use regex::Regex;
const BUFFER_CAPACITY: usize = 1024 * 1024 * 5;

/// Parsing log files using the provided configuration.
///
/// Processes each line of the log file, extracting
/// fields using a regex patterns and constructing data tuples based on a parsing schema.
///
/// # Arguments
/// * `input_file` - Path to the log file to be parsed
/// * `run_config` - Configuration for the parsing run
/// * `metadata` - Metadata associated with the input file
/// * `log_config_file` - Path to the configuration file defining the parsing schema
/// * `log_before_fail` - Number of errors to log before failing
///
#[pyfunction]
pub fn parse_regexp(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: usize,
) -> RunReport {
    match parse(
        input_file,
        run_config,
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

/// Core parsing logic for log files using a configuration schema.
///
/// This function implements the detailed parsing process, including
/// regex matching, field parsing, and output generation.
///
/// # Arguments
/// * `input_file` - Path to the log file to be parsed
/// * `run_config` - Configuration for the parsing run
/// * `metadata` - Metadata associated with the input file
/// * `csv_config_file` - Path to the configuration file defining the parsing schema
/// * `log_before_fail` - Number of errors to log before failing
///
fn parse(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: usize,
) -> Result<RunReport, Error> {
    let data_mapping = &plugin_config.get_data_type_mapping(None)?;
    let field_mapping = data_mapping
        .field_mapping
        .clone()
        .ok_or(Error::CannotBeEmpty("FieldMapping".to_owned()))?;

    //retrieve specific configuration
    let skip_lines: usize = data_mapping
        .params
        .get("skip_lines")
        .cloned()
        .unwrap_or("0".to_string())
        .parse()?;

    let regex = data_mapping
        .params
        .get("regex")
        .cloned()
        .ok_or(Error::ConfigurationError(
            "'regex' is not set in the configuration ".to_string(),
        ))?;

    let policy = data_mapping
        .params
        .get("regexp_fail_policy")
        .ok_or(Error::ConfigurationError(
            "'regexp_fail_policy' is not set in the configuration ".to_string(),
        ))?;
    let fail_policy = FailPolicy::from_string(policy)?;

    //get the last field parameters for the 'Merge' policy
    let last_field = field_mapping
        .last_field
        .as_ref()
        .ok_or(Error::ConfigurationError(
            "Invalid mapping, at least one field must be declared".to_string(),
        ))?;

    let last_field_name = if let Field::Single {
        name,
        parser: _,
        default_value: _,
    } = last_field
    {
        name.input_name()
    } else {
        return Err(Error::ConfigurationError(
            "last field must be a normal field (not an array, an object, etc.) ".to_string(),
        ));
    };

    if let FailPolicy::Merge = fail_policy {
        if let Field::Single {
            name: _,
            parser,
            default_value: _,
        } = last_field
            && let Parser::String() = parser
        {
        } else {
            return Err(Error::ConfigurationError(
                "For the 'Merge' policy, last field must be have a String parser".to_string(),
            ));
        }
    }

    // start the parsing
    let decode_builder = encoding_reader_builder(&plugin_config.file_encoding)?;
    let file_handle = File::open(input_file)
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;
    let decode_reader = decode_builder.build(file_handle);

    let mut reader = BufReader::with_capacity(BUFFER_CAPACITY, decode_reader);

    //Compile the regex pattern from the configuration
    let re = Regex::new(&regex)?;
    let mut output = Output::new(run_config, plugin_config, metadata, None)?;

    //build a field parser map because the captured values by the regexp dannot be interated over ther name
    let mut field_map: HashMap<String, FieldParser> = HashMap::new();
    for (key, parser_type) in &field_mapping.field_parser_tree.parsers {
        if let ParserType::Field { parser } = parser_type {
            field_map.insert(key.to_string(), parser.clone());
        }
    }

    //Initialize tracking variables for line processing
    let mut run_report = RunReport::new();
    let mut line = String::new();
    let mut record = Record::new();
    let mut line_number = 0;

    while reader.read_line(&mut line)? > 0 {
        if line_number >= skip_lines
            && let Err(e) = build_record(
                last_field_name,
                &mut field_map,
                &regex,
                &re,
                &mut output,
                &line,
                &mut record,
                line_number,
                &fail_policy,
            )
        {
            run_report.add_error(e.to_string());
            record.clear();
            if run_report.num_errors >= log_before_fail {
                break;
            }
        }

        line_number += 1;
        line.clear();
    }
    // Write any remaining data in the record to output
    if !record.0.is_empty() {
        output.write(&mut record)?;
    }
    run_report.add_output_report(output.get_report());
    Ok(run_report)
}

/// Builds a data record from a log line using regex captures.
///
/// This function processes each line of the log file, extracting
/// fields using regex patterns and constructing data records.
///
/// # Arguments
/// * `last_field` - Name of the last field in the schema
/// * `field_map` - Mapping of field names to their parsers
/// * `regex` - Regex pattern used for matching
/// * `re` - Compiled regex object
/// * `output` - Output handler for writing parsed data
/// * `line` - Current line being processed
/// * `record` - Current data `Record` being built
/// * `line_number` - Current line number in the input file
///
#[allow(clippy::too_many_arguments)]
fn build_record(
    last_field: &str,
    field_map: &mut HashMap<String, FieldParser>,
    regex: &str,
    re: &Regex,
    output: &mut Output,
    line: &str,
    record: &mut Record,
    line_number: usize,
    fail_policy: &FailPolicy,
) -> Result<(), Error> {
    if let Some(caps) = re.captures(line) {
        if !record.0.is_empty() {
            output.write(record)?;
        }
        for (key, field) in field_map {
            match caps.name(key) {
                Some(input) => field.parse(Some(input.as_str()), record)?,
                None => field.set_value(Value::Null(), record)?,
            }
        }
    } else {
        match fail_policy {
            FailPolicy::Skip => return Ok(()),
            FailPolicy::Fail => {
                return Err(Error::InvalidLogRegex(line_number, regex.to_owned()));
            }
            FailPolicy::Merge => {
                if record.0.is_empty() {
                    return Err(Error::InvalidLogRegex(line_number, regex.to_owned()));
                } else {
                    match record.0.get_mut(last_field) {
                        Some(v) => {
                            if let Value::String(last_field_value) = v {
                                last_field_value.push('\n');
                                last_field_value.push_str(line.trim());
                            } //allways true, last_field is checked to be a String before
                        }
                        None => {
                            record.add(last_field, Value::String(line.trim().to_owned()));
                        }
                    }
                }
            }
        }
    };
    Ok(())
}

#[derive(Clone, Debug)]
pub enum FailPolicy {
    Skip,
    Fail,
    Merge,
}
impl FailPolicy {
    pub fn from_string(policy: &str) -> Result<Self, Error> {
        match policy {
            "Skip" => Ok(Self::Skip),
            "Fail" => Ok(Self::Fail),
            "Merge" => Ok(Self::Merge),
            _ => Err(Error::ConfigurationError(
                "Invalid policy, expecting : 'Skip', 'Fail' or 'Merge'".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {

    use std::{fs, path::Path};

    use crate::{
        DateInputCodec, FieldMapping, FieldName, OutputConfiguration,
        configuration::DataTypeMapping,
    };

    use super::*;

    fn remove_dir_if_exists(path: &Path) {
        match fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }

    fn regexp_output_conf(base_file_name: &str, output_folder: &Path) -> OutputConfiguration {
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

    fn regexp_plugin_config(regex: &str, fail_policy: &str) -> PluginConfiguration {
        let mut params = HashMap::new();
        params.insert("regex".to_owned(), regex.to_owned());
        params.insert("regexp_fail_policy".to_owned(), fail_policy.to_owned());
        PluginConfiguration {
            plugin: "regexp-test".to_string(),
            file_encoding: "UTF_8".to_string(),
            data_type_configs: vec![DataTypeMapping {
                data_type: "mini_log".to_string(),
                description: None,
                default_date_pattern: DateInputCodec::Iso(),
                params,
                timeline: None,
                field_mapping: Some(FieldMapping::new(
                    vec![
                        Field::Single {
                            name: FieldName::new("date".to_owned(), false, None, None, None),
                            parser: Parser::String(),
                            default_value: None,
                        },
                        Field::Single {
                            name: FieldName::new("level".to_owned(), false, None, None, None),
                            parser: Parser::String(),
                            default_value: None,
                        },
                        Field::Single {
                            name: FieldName::new(
                                "message".to_owned(),
                                false,
                                None,
                                None,
                                None,
                            ),
                            parser: Parser::String(),
                            default_value: None,
                        },
                    ],
                    None,
                )),
                has_primary_key: false,
            }],
        }
    }

    #[test]
    fn optional_capture_group_sets_null_when_missing() {
        let output_folder = Path::new(".tmp").join("regexp_optional_capture");
        remove_dir_if_exists(&output_folder);
        fs::create_dir_all(&output_folder).unwrap();
        let input_path = output_folder.join("optional.log");
        fs::write(&input_path, "2026-01-01 INFO started\n2026-01-02 stopped\n").unwrap();
        let regex = r"^(?P<date>\d{4}-\d{2}-\d{2}) (?:(?P<level>INFO|WARN) )?(?P<message>.*)";
        let run_config = RunConfiguration::new(
            vec![regexp_output_conf("optional", &output_folder)],
            true,
            None,
        );

        let report = parse_regexp(
            input_path.to_str().unwrap(),
            run_config,
            regexp_plugin_config(regex, "Fail"),
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        let jsonl = fs::read_to_string(output_folder.join("optional.mini_log.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0]["level"], "INFO");
        assert!(lines[1]["level"].is_null());
        assert_eq!(lines[1]["message"], "stopped");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn merge_policy_reports_leading_continuation_and_keeps_later_records() {
        let output_folder = Path::new(".tmp").join("regexp_merge_leading");
        remove_dir_if_exists(&output_folder);
        fs::create_dir_all(&output_folder).unwrap();
        let input_path = output_folder.join("merge.log");
        fs::write(
            &input_path,
            "orphan continuation\n2026-01-01 INFO started\ncontinued details\n2026-01-02 WARN next\n",
        )
        .unwrap();
        let regex = r"^(?P<date>\d{4}-\d{2}-\d{2}) (?P<level>INFO|WARN) (?P<message>.*)";
        let run_config = RunConfiguration::new(
            vec![regexp_output_conf("merge", &output_folder)],
            true,
            None,
        );

        let report = parse_regexp(
            input_path.to_str().unwrap(),
            run_config,
            regexp_plugin_config(regex, "Merge"),
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.num_errors, 1);
        assert!(report.last_error.as_ref().unwrap().contains("line:'0'"));
        let jsonl = fs::read_to_string(output_folder.join("merge.mini_log.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["message"], "started\ncontinued details");
        assert_eq!(lines[1]["message"], "next");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn skip_policy_ignores_unmatched_lines() {
        let output_folder = Path::new(".tmp").join("regexp_skip_policy");
        remove_dir_if_exists(&output_folder);
        fs::create_dir_all(&output_folder).unwrap();
        let input_path = output_folder.join("skip.log");
        fs::write(
            &input_path,
            "unmatched line\n2026-01-01 INFO kept\nanother unmatched line\n",
        )
        .unwrap();
        let regex = r"^(?P<date>\d{4}-\d{2}-\d{2}) (?P<level>INFO|WARN) (?P<message>.*)";
        let run_config =
            RunConfiguration::new(vec![regexp_output_conf("skip", &output_folder)], true, None);

        let report = parse_regexp(
            input_path.to_str().unwrap(),
            run_config,
            regexp_plugin_config(regex, "Skip"),
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 1);
        let jsonl = fs::read_to_string(output_folder.join("skip.mini_log.jsonl")).unwrap();
        let line: serde_json::Value = serde_json::from_str(jsonl.trim()).unwrap();
        assert_eq!(line["message"], "kept");

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn invalid_fail_policy_is_reported() {
        let output_folder = Path::new(".tmp").join("regexp_invalid_policy");
        remove_dir_if_exists(&output_folder);
        fs::create_dir_all(&output_folder).unwrap();
        let input_path = output_folder.join("invalid_policy.log");
        fs::write(&input_path, "2026-01-01 INFO ignored\n").unwrap();
        let regex = r"^(?P<date>\d{4}-\d{2}-\d{2}) (?P<level>INFO|WARN) (?P<message>.*)";
        let run_config = RunConfiguration::new(
            vec![regexp_output_conf("invalid_policy", &output_folder)],
            true,
            None,
        );

        let report = parse_regexp(
            input_path.to_str().unwrap(),
            run_config,
            regexp_plugin_config(regex, "Bogus"),
            Metadata::new("test".into()),
            1000,
        );

        assert!(
            report
                .last_error
                .as_ref()
                .unwrap()
                .contains("Invalid policy")
        );
        assert_eq!(report.num_errors, 1);

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn log_parse() {
        let targetfile = ".tmp/log_win2k.win2k.jsonl";
        if Path::new(targetfile).exists() {
            let _ = fs::remove_file(targetfile);
        }

        let output_conf = OutputConfiguration::new(
            "log_win2k".to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);
        let xml = fs::read_to_string("test_data/logs/win2k.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_regexp(
            "test_data/logs/win2k.log",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
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

        let usn = lines[0]
            .as_object()
            .unwrap()
            .get("timestamp")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(usn, "2016-09-28T04:30:30.000000+00:00");

        let usn = lines[4]
            .as_object()
            .unwrap()
            .get("message")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(usn, "Ending TrustedInstaller initialization.");
    }

    #[test]
    fn log_parse_multiline() {
        let targetfile = ".tmp/log_win2k_multi.win2k.jsonl";
        if Path::new(targetfile).exists() {
            let _ = fs::remove_file(targetfile);
        }

        let output_conf = OutputConfiguration::new(
            "log_win2k_multi".to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);
        let xml = fs::read_to_string("test_data/logs/win2k.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_regexp(
            "test_data/logs/win2k_multi_line.log",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
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

        let usn = lines[5]
            .as_object()
            .unwrap()
            .get("message")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(
            usn,
            "Starting the TrustedInstaller main loop.\nwith some\nadditional\nlines"
        );
    }

    #[test]
    fn log_parse_last_field_error() {
        let targetfile = ".tmp/log_win2k_multi_parse_last.win2k.jsonl";
        if Path::new(targetfile).exists() {
            let _ = fs::remove_file(targetfile);
        }

        let output_conf = OutputConfiguration::new(
            "log_win2k_multi".to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);

        let xml = fs::read_to_string("test_data/logs/win2k_last_field_error.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_regexp(
            "test_data/logs/win2k_multi_line.log",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );
        if let Some(e) = &report.last_error {
            assert_eq!(
                e,
                "'For the 'Merge' policy, last field must be have a String parser'"
            )
        } else {
            panic!("must return an error")
        }
    }

    #[test]
    fn skip_lines() {
        let targetfile = ".tmp/log_ngen.ngen.jsonl";
        if Path::new(targetfile).exists() {
            fs::remove_file(targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            "log_ngen".to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);
        let xml = fs::read_to_string("test_data/logs/ngen.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = {
            parse_regexp(
                "test_data/logs/ngen.log",
                run_config,
                plugin_config,
                Metadata::new("test".into()),
                0,
            )
        };

        assert_eq!(report.last_error, None);
        let expected_lines = 12;
        let result = &report.output_reports[0];
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());

        let pid = lines[5]
            .as_object()
            .unwrap()
            .get("process_id")
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(pid, 64);
    }
}
