use crate::{
    FieldParserTree, Metadata, Output, Parser, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration,
    errors::Error,
    field::MultiInputContext,
    parser::{json::parse_json_object, runner},
};
use chrono::DateTime;
use evtx::{EvtxParser, ParserSettings, SerializedEvtxRecord};
use pyo3::prelude::*;
use serde_json::Value as JsonValue;

const TIMESTAMP_FIELD: &str = "timestamp";

#[pyfunction]
pub fn parse_evtx(
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
    runner::validate_configuration(&run_config, &plugin_config)?;

    let data_type_config = &plugin_config.get_data_type_mapping(None)?;
    let parser_tree = data_type_config
        .field_mapping
        .as_ref()
        .ok_or(Error::ConfigurationError(
            "There is no field mapping in the configuration".to_string(),
        ))?
        .get_field_parser_tree();

    let settings = ParserSettings::new().separate_json_attributes(true);
    let parser: EvtxParser<std::fs::File> = EvtxParser::from_path(input_file)?;
    let mut parser = parser.with_configuration(settings);

    let mut report = RunReport::new();
    let output = Output::new(run_config, plugin_config, metadata, None)?;
    let mut evt_number = 0;
    let mut tuple = Record::new();
    runner::with_output(output, &mut report, |output, report| {
        for record in parser.records_json_value() {
            evt_number += 1;
            match record {
                Ok(rec) => {
                    let timestamp_micro = rec.timestamp.as_microsecond();
                    if let Some(dt) = DateTime::from_timestamp_micros(timestamp_micro) {
                        tuple.add(TIMESTAMP_FIELD, Value::Date(dt));
                    };

                    parse_record(rec, &mut tuple, output, &parser_tree, report);
                }
                Err(e) => report.add_error(format!("{e}, Event number: {evt_number}")),
            };
            tuple.clear();
        }
        Ok(())
    });
    Ok(report)
}

fn parse_record(
    record: SerializedEvtxRecord<JsonValue>,
    tuple: &mut Record,
    output: &mut Output,
    parser_tree: &FieldParserTree,
    report: &mut RunReport,
) {
    let parse_unknown = if let Some(Parser::Ignore()) = parser_tree.default_parser {
        false
    } else {
        !parser_tree.ignore_parsing
    };

    match get_event(record) {
        Ok(json) => {
            if let JsonValue::Object(map) = json {
                let mut multi_input_context =
                    MultiInputContext::new(parser_tree.multi_input_group_count);
                match parse_json_object(
                    map,
                    Some(parser_tree),
                    parse_unknown,
                    tuple,
                    &mut multi_input_context,
                ) {
                    Ok(()) => {
                        if let Err(e) = output.write(tuple) {
                            report.add_error(e.to_string());
                        }
                    }
                    Err(e) => report.add_error(e.to_string()),
                }
            }
        }
        Err(e) => report.add_error(e.to_string()),
    }
}

fn get_event(mut record: SerializedEvtxRecord<JsonValue>) -> Result<JsonValue, Error> {
    let event = record
        .data
        .as_object_mut()
        .ok_or(Error::JsonNotAnObject("this record".to_owned()))?
        .remove("Event")
        .ok_or(Error::EvtxError("'Event' data not found".to_string()))?;
    Ok(event)
}

#[cfg(test)]
mod tests {
    use crate::{
        DateInputCodec, Field, FieldMapping, FieldName, OutputConfiguration, Parser,
        TimeLineBuilder, TimeLineType, configuration::DataTypeMapping,
    };

    use super::*;
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

    use serde_json::Value;

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

    fn evtx_nested_system_time_plugin_config() -> PluginConfiguration {
        let field_mapping = FieldMapping::new(
            vec![
                Field::Single {
                    name: FieldName::new("timestamp".to_owned(), false, None, None, None),
                    parser: Parser::Ignore(),
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
                    fields: vec![Field::Object {
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
                                Some("Nested EVTX System Time".to_owned()),
                                None,
                            ),
                            parser: Parser::DateTime(DateInputCodec::Iso()),
                            default_value: None,
                        }],
                    }],
                },
            ],
            Some(Parser::Ignore()),
        );

        PluginConfiguration {
            plugin: "evtx-test".to_string(),
            file_encoding: "UTF_8".to_string(),
            data_type_configs: vec![DataTypeMapping {
                data_type: "evtx_nested_time".to_string(),
                description: None,
                default_date_pattern: DateInputCodec::Iso(),
                params: HashMap::new(),
                timeline: Some(TimeLineBuilder::new(
                    TimeLineType::Standard,
                    "evtx_nested_time".to_string(),
                    0,
                    None,
                    None,
                )),
                field_mapping: Some(field_mapping),
                has_primary_key: false,
            }],
        }
    }

    #[test]
    fn evtx_nested_system_time_string_mapping_emits_timeline_record() {
        let output_folder = test_folder("evtx_nested_system_time_string_timeline");
        let output_conf = OutputConfiguration::new(
            "evtx_nested_time".to_string(),
            output_folder.display().to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            HashMap::new(),
        );
        let configuration = RunConfiguration::new(vec![output_conf], true, None);
        let plugin_config = evtx_nested_system_time_plugin_config();

        let report = parse_evtx(
            "test_data/evtx/kernel_pnp.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports[0].file_reports[0].num_lines, 123);

        let jsonl =
            fs::read_to_string(output_folder.join("evtx_nested_time.evtx_nested_time.jsonl"))
                .unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 123);

        let line = lines[3].as_object().unwrap();
        assert_eq!(
            line.get("timestamp").unwrap().as_str().unwrap(),
            "2016-04-26T21:30:13.589912+00:00"
        );
        assert_eq!(
            line.get("timestamp_meaning").unwrap().as_str().unwrap(),
            "Nested EVTX System Time"
        );
        assert_eq!(
            line.get("data").unwrap()["system"]["time_created"]["system_time"],
            "2016-04-26T21:30:13.589912+00:00"
        );

        remove_dir_if_exists(&output_folder);
    }

    #[test]
    fn evtx() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_kernel_pnp";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let metadata = Metadata::new("test".into());

        let report = parse_evtx(
            "test_data/evtx/kernel_pnp.evtx",
            configuration,
            plugin_config,
            metadata,
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(123, lines.len());

        let line = lines[3].as_object().unwrap();
        let user = line
            .get("system")
            .unwrap()
            .as_object()
            .unwrap()
            .get("security")
            .unwrap()
            .get("user_id")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(user, "S-1-5-18");
    }

    #[test]
    fn evtx_analytics_jsonl_writes_joinable_rows() {
        let output_folder = test_folder("analytics");
        let base_file_name = "evtx_kernel_pnp_analytics";
        let data_path = output_folder.join(format!("{base_file_name}.evtx.jsonl"));
        let metadata_path = output_folder.join("ogre_metadata.jsonl");
        let timeline_path = output_folder.join(format!("{base_file_name}.evtx.tl.jsonl"));

        let output_conf = OutputConfiguration::new(
            base_file_name.to_owned(),
            output_folder.display().to_string(),
            "file".to_owned(),
            "analytics_jsonl".to_owned(),
            "iso".to_owned(),
            true,
            false,
            HashMap::new(),
        );
        let configuration = RunConfiguration::new(vec![output_conf], true, None);
        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/kernel_pnp.evtx",
            configuration,
            plugin_config,
            Metadata::new("analytics-test-host".into()),
        );

        assert_eq!(report.last_error, None);
        assert_eq!(report.output_reports.len(), 1);
        let file_reports = &report.output_reports[0].file_reports;
        assert_eq!(file_reports.len(), 3);
        assert!(
            file_reports
                .iter()
                .all(|file_report| file_report.format == "analytics_jsonl")
        );

        let read_jsonl = |path: &Path| -> Vec<Value> {
            fs::read_to_string(path)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        };
        let data_rows = read_jsonl(&data_path);
        let metadata_rows = read_jsonl(&metadata_path);
        let timeline_rows = read_jsonl(&timeline_path);

        assert_eq!(data_rows.len(), 123);
        assert_eq!(metadata_rows.len(), 1);
        assert_eq!(timeline_rows.len(), 123);

        let data_row = &data_rows[3];
        let data_id = data_row["ogre_id"].as_str().expect("analytics data id");
        let metadata_id = data_row["ogre_md_id"]
            .as_str()
            .expect("analytics metadata id");
        assert!(!data_id.is_empty());
        assert!(!metadata_id.is_empty());
        assert_eq!(data_row["computer"].as_str(), Some("analytics-test-host"));
        assert_eq!(data_row["data_type"].as_str(), Some("evtx"));
        assert_eq!(
            data_row["data"]["system"]["computer"].as_str(),
            Some("MINWINPC")
        );
        assert!(data_row.get("system").is_none());

        let metadata_row = &metadata_rows[0];
        assert_eq!(metadata_row["id"].as_str(), Some(metadata_id));
        assert_eq!(
            metadata_row["computer"].as_str(),
            Some("analytics-test-host")
        );
        assert_eq!(metadata_row["data_type"].as_str(), Some("evtx"));

        let timeline_row = timeline_rows
            .iter()
            .find(|row| row["data_id"].as_str() == Some(data_id))
            .expect("timeline row linked to representative data row");
        assert_eq!(timeline_row["data_id"].as_str(), Some(data_id));
        assert_eq!(timeline_row["metadata_id"].as_str(), Some(metadata_id));
        assert_eq!(
            timeline_row["computer"].as_str(),
            Some("analytics-test-host")
        );
        assert_eq!(timeline_row["data_type"].as_str(), Some("evtx"));
    }

    #[test]
    fn test_issue_6() {
        let input_file = "test_data/evtx/w7_evtx_system.evtx";
        let parser: EvtxParser<std::fs::File> = EvtxParser::from_path(input_file).unwrap();
        let settings = ParserSettings::new().separate_json_attributes(true);
        let mut parser = parser.with_configuration(settings);
        let mut line_number = 0;
        for record in parser.records_json_value() {
            // let mut record = record.unwrap();
            line_number += 1;
            let erroneous_line = 1111;
            if record.is_err() {
                assert_eq!(line_number, erroneous_line)
            } else if line_number == erroneous_line {
                panic!("Line {erroneous_line} should be in error")
            }
        }
    }

    #[test]
    fn test_windows_powershell() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_windows_powershell";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            output_folder.to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let metadata = Metadata::new("test".into());

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/windows_powershell.evtx",
            configuration,
            plugin_config,
            metadata,
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(182, lines.len());

        let line = lines[3].as_object().unwrap();
        let description = line.get("description").unwrap().as_str().unwrap();
        assert_eq!(description, "PowerShell:600");

        let name = line
            .get("data")
            .unwrap()
            .as_object()
            .unwrap()
            .get("system")
            .unwrap()
            .as_object()
            .unwrap()
            .get("provider")
            .unwrap()
            .get("provider_name")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(name, "PowerShell");
    }

    #[test]
    fn test_timeline_evtx() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_timeline";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            output_folder.to_string(),
            "file".to_string(),
            "jsonl".to_string(),
            "iso".to_string(),
            true,
            true,
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let metadata = Metadata::new("test".into());

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/kernel_pnp.evtx",
            configuration,
            plugin_config,
            metadata,
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(123, lines.len());
        //"relatedUser":"S-1-5-18","description":"name: Microsoft-Windows-Kernel-PnP, event_id: 410","additionalDescription":"event_record_id: 4, computer: MINWINPC"

        let line = lines[3].as_object().unwrap();

        let user = line.get("related_user").unwrap().as_str().unwrap();
        assert_eq!(user, "S-1-5-18");

        let description = line.get("description").unwrap().as_str().unwrap();
        assert_eq!(description, "Microsoft-Windows-Kernel-PnP:410");

        let additional_description = line
            .get("additional_description")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(
            additional_description,
            "device_instance_id: ACPI_HAL\\PNP0C08\\0 - driver_name: acpi.inf"
        );

        let date = line
            .get("data")
            .unwrap()
            .as_object()
            .unwrap()
            .get("timestamp")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(date, "2016-04-26T21:30:13.589912+00:00");

        //test otherwise additional description
        let line = lines[5].as_object().unwrap();
        let additional_description = line
            .get("additional_description")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(
            additional_description,
            "event_data: 'DeviceInstanceId':'ACPI\\PNP0C01\\1', 'DriverName':'machine.inf', 'ClassGuid':'4D36E97D-E325-11CE-BFC1-08002BE10318', 'DriverDate':'06/21/2006', 'DriverVersion':'10.0.10586.0', 'DriverProvider':'Microsoft', 'DriverInbox':'true', 'DriverSection':'NO_DRV_MBRES', 'DriverRank':'0xff0002', 'MatchingDeviceId':'*PNP0C01', 'OutrankedDrivers':'', 'DeviceUpdated':'false', 'Status':'0x0', 'ParentDeviceInstanceId':'ACPI_HAL\\PNP0C08\\0'"
        );
    }

    #[test]
    fn security_windows() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_security_windows";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/Security_windows1122H2.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(10440, lines.len());

        let line = lines[3].as_object().unwrap();
        let date = line.get("timestamp").unwrap().as_str().unwrap();
        assert_eq!(date, "2022-09-24T01:01:13.625085+00:00");
    }

    #[test]
    fn system_hive_with_service_events() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_system_hive_with_service_events";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/system_hive_with_service_events.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(6109, lines.len());

        let line = lines[3].as_object().unwrap();
        let date = line.get("timestamp").unwrap().as_str().unwrap();
        assert_eq!(date, "2022-09-24T01:01:13.586750+00:00");
    }

    #[test]
    fn test_evt_4697() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_test_evt_4697";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/test_evt_4697.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(2963, lines.len());

        let line = lines[3].as_object().unwrap();
        let date = line.get("timestamp").unwrap().as_str().unwrap();
        assert_eq!(date, "2019-07-22T13:35:20.992943+00:00");
    }

    #[test]
    fn test_evt_7045() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_test_evt_7045";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/test_evt_7045.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(571, lines.len());

        let line = lines[3].as_object().unwrap();
        let date = line.get("timestamp").unwrap().as_str().unwrap();
        assert_eq!(date, "2021-11-30T12:33:56.592357+00:00");
    }

    #[test]
    fn test_evt_7045bis() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_test_evt_7045bis";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/test_evt_7045bis.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(1557, lines.len());

        let line = lines[3].as_object().unwrap();
        let date = line.get("timestamp").unwrap().as_str().unwrap();
        assert_eq!(date, "2019-07-22T13:35:37.772777+00:00");
    }

    #[test]
    fn w7_evtx_system() {
        let output_folder = ".tmp";
        let base_file_name = "evtx_w7_evtx_system";
        let targetfile = format!("{output_folder}/{base_file_name}.evtx.jsonl");
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
            HashMap::new(),
        );

        let configuration = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/evtx/evtx.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_evtx(
            "test_data/evtx/w7_evtx_system.evtx",
            configuration,
            plugin_config,
            Metadata::new("test".into()),
        );

        assert_eq!(report.last_error, None);

        assert_eq!(1110, report.output_reports[0].file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(1110, lines.len());

        let line = lines[3].as_object().unwrap();
        let date = line.get("timestamp").unwrap().as_str().unwrap();
        assert_eq!(date, "2009-07-14T05:14:24.168612+00:00");
    }
}
