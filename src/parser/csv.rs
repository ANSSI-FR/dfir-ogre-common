use std::{fs::File, io::Read, path::Path};

use pyo3::prelude::*;

use crate::{
    Error, FieldParser, Metadata, Output, Record, RunConfiguration, RunReport,
    configuration::{PluginConfiguration, encoding_reader_builder},
};
const BUFFER_CAPACITY: usize = 1024 * 1024 * 5;

/// This function reads a CSV file and processes it according to the specified
/// run configuration, metadata, and CSV configuration file. It can use Python
/// mappings to transform data during parsing.
///
/// # Arguments
///
/// * `input_file` - Path to the input CSV file to be parsed
/// * `run_config` - Configuration settings for the parsing run
/// * `metadata` - Metadata associated with the parsing operation
/// * `csv_config_file` - Path to the CSV mapping file
/// * `python_mapping` - A mapping of column names to Python functions for data transformation
/// * `log_before_fail` - Number of rows to log before returning an error
///
/// # Returns
///
/// A `RunReport` containing the results of the parsing operation,
/// including any errors that occurred during processing.
///
#[pyfunction]
pub fn parse_csv(
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
/// Parses a CSV file according to the provided configuration and mappings.
/// # Returns
///
/// A `Result<RunReport, Error>` containing the parsing results or an error if something went wrong.
///
fn parse(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
    log_before_fail: usize,
) -> Result<RunReport, Error> {
    let file_handle = File::open(Path::new(input_file))
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

    let reader_builder = encoding_reader_builder(&plugin_config.file_encoding)?;
    let decode_builder = reader_builder.build(file_handle);

    let data_type_config = &plugin_config.get_data_type_mapping(None)?;

    let csv_delimiter =
        data_type_config
            .params
            .get("csv_delimiter")
            .ok_or(Error::ConfigurationError(format!(
                "csv_delimiter is not set in the configuration "
            )))?;

    let csv_delimiter: char = csv_delimiter
        .chars()
        .next()
        .ok_or(Error::ConfigurationError(format!(
            "csv_delimiter is empty "
        )))?;

    let mut reader: csv::Reader<Box<dyn Read>> = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .delimiter(csv_delimiter as u8)
        .buffer_capacity(BUFFER_CAPACITY)
        .from_reader(Box::new(decode_builder));

    let headers: &csv::StringRecord = reader.headers()?;

    let mut converters: Vec<Option<FieldParser>> = Vec::with_capacity(headers.len());

    let mut field_parsers = data_type_config
        .field_mapping
        .clone()
        .ok_or(Error::ConfigurationError(format!(
            "field mapping is empty "
        )))?
        .get_field_parser_tree();

    for field_name in headers.iter() {
        converters.push(field_parsers.get_parser(field_name));
    }

    let mut output = Output::new(run_config, plugin_config, metadata, None)?;
    let mut run_report = RunReport::new();
    parse_lines(
        reader,
        &mut converters,
        &mut output,
        log_before_fail,
        &mut run_report,
    );
    run_report.add_output_report(output.get_report());

    Ok(run_report)
}

/// Parses lines from a CSV reader using the provided converters and outputs the results.
///
/// This function iterates through each line of the CSV file, parses the data according to
/// the given converters, and writes it to the output. It handles errors gracefully by logging
/// them or returning an error based on `log_before_fail`.
///
/// # Arguments
///
/// * `reader` - A mutable reference to the CSV reader to read lines from.
/// * `converters` - A mutable vector of field parsers used to convert each field.
/// * `output` - A mutable reference to the output handler where parsed data is written.
/// * `log_before_fail` - The number of errors to log before returning an error.
///
fn parse_lines(
    mut reader: csv::Reader<Box<dyn Read>>,
    converters: &mut [Option<FieldParser>],
    output: &mut Output,
    log_before_fail: usize,
    run_report: &mut RunReport,
) {
    let mut tuple = Record::new();

    for (line_nb, record) in reader.records().enumerate() {
        match record {
            Ok(string_record) => {
                for (column, data) in string_record.iter().enumerate() {
                    match &mut converters[column] {
                        Some(converter) => {
                            if let Err(e) = converter.parse(Some(data), &mut tuple) {
                                let err = Error::ParsingColumn(line_nb + 1, e.to_string(), column);
                                run_report.add_error(err.to_string());
                                if run_report.num_errors > log_before_fail {
                                    return;
                                }
                            }
                        }
                        None => continue,
                    }
                }
                if let Err(e) = output.write(&mut tuple) {
                    let err = Error::ParsingLine(line_nb + 1, e.to_string());
                    tuple.clear();
                    run_report.add_error(err.to_string());
                    if run_report.num_errors > log_before_fail {
                        break;
                    }
                }
            }
            Err(e) => {
                let err = Error::ParsingLine(line_nb + 1, e.to_string());
                tuple.clear();
                run_report.add_error(err.to_string());
                if run_report.num_errors > log_before_fail {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, fs, path::Path};

    use serde_json::Value;

    use crate::{
        OutputConfiguration,
        field::ParserExtension,
        format_csv::CSV_DELIMITER,
        parser::windows_parsers::{win_frn_hex_parser, win_ntfs_flag_parser},
        win_signed_hash_parser,
    };

    use super::*;

    /// Tests the parsing of an NTFSInfo CSV file.
    ///
    /// It ensures that:
    /// - No errors are returned during parsing (i.e., `report.last_error` is `None`)
    /// - The correct number of lines are parsed (3)
    /// - The output JSONL file contains 3 lines
    /// - Specific values from the first, second, and third lines are correctly extracted
    #[test]
    fn ntfs_info_parse() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.jsonl");

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

        let run_config = RunConfiguration::new(vec![output_conf], false, None);

        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());

        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        let result = &report.output_reports[0];

        let expected_lines = 3;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());

        let usn = lines[0]
            .as_object()
            .unwrap()
            .get("usn_number")
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(usn, 72817800);

        let file_path = lines[1]
            .as_object()
            .unwrap()
            .get("file_path")
            .unwrap()
            .as_str()
            .unwrap();

        assert!(matches!(file_path, "\\$Bitmap"));

        let fn_creation_date = lines[2]
            .as_object()
            .unwrap()
            .get("fn_creation_date")
            .unwrap()
            .as_str()
            .unwrap();

        assert!(matches!(
            fn_creation_date,
            "2016-01-22T03:08:51.337000+00:00"
        ));
    }

    #[test]
    fn ntfs_info_parse_csv() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo_csv";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.csv");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "csv".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);

        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());

        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        let result = &report.output_reports[0];

        let expected_lines = 3;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .flexible(false)
            .from_path(targetfile)
            .unwrap();

        let mut line_number = 0;
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();

            if line_number == 1 {
                for (num, v) in rec.iter().enumerate() {
                    if num == 54 {
                        println!("{v}");
                        assert_eq!(v, "[\"hello\",\"world\"]")
                    }
                }
            }
            assert_eq!(63, rec.len());
        }
        assert_eq!(3, line_number);
    }

    #[test]
    fn ntfs_info_parse_timeline_csv() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo_timeline_csv";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.csv");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "csv".to_string(),
            "iso".to_string(),
            true,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);

        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());

        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        let result = &report.output_reports[0];

        let expected_lines = 5;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .flexible(false)
            .from_path(targetfile)
            .unwrap();

        let mut line_number = 0;
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();

            if line_number == 1 {
                for (num, v) in rec.iter().enumerate() {
                    if num == 54 {
                        println!("{v}");
                        assert_eq!(v, "[\"hello\",\"world\"]")
                    }
                }
            }
            assert_eq!(9, rec.len());
        }
        assert_eq!(5, line_number);
    }

    /// Tests the parsing of an NTFSInfo CSV file with timeline output.
    ///
    /// - No errors are returned during parsing (i.e., `report.last_error` is `None`)
    /// - The correct number of lines are parsed (5)
    /// - The output JSONL file contains 5 lines
    /// - Specific timestamp values, meanings, and descriptions from the timeline output are correctly extracted
    #[test]
    fn ntfs_info_timeline_parse() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo_timeline";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.jsonl");

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
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);
        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());
        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        let result = &report.output_reports[0];

        let expected_lines = 5;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(expected_lines, lines.len());

        let mut parsed = vec![];

        //first line
        let line = lines[0].as_object().unwrap();
        let timestamp = line.get("timestamp").unwrap().as_str().unwrap();
        let meaning = line.get("timestamp_meaning").unwrap().as_str().unwrap();
        let description = line.get("description").unwrap().as_str().unwrap();
        parsed.push((timestamp, meaning, description));

        //second line
        let line = lines[1].as_object().unwrap();
        let timestamp = line.get("timestamp").unwrap().as_str().unwrap();
        let meaning = line.get("timestamp_meaning").unwrap().as_str().unwrap();
        let description = line.get("description").unwrap().as_str().unwrap();
        parsed.push((timestamp, meaning, description));

        //third line
        let line = lines[2].as_object().unwrap();
        let timestamp = line.get("timestamp").unwrap().as_str().unwrap();
        let meaning = line.get("timestamp_meaning").unwrap().as_str().unwrap();
        let description = line.get("description").unwrap().as_str().unwrap();
        parsed.push((timestamp, meaning, description));

        for (timestamp, meaning, description) in parsed {
            if timestamp.eq("2016-01-22T03:08:51.337000+00:00") {
                assert_eq!(meaning, "$SI:.... - $FN:MACB");
                assert_eq!(description, "file_path: \\.");
            } else if timestamp.eq("2015-10-30T06:28:30.642000+00:00") {
                assert_eq!(meaning, "$SI:...B - $FN:....");
                assert_eq!(description, "file_path: \\.");
            } else if timestamp.eq("2016-02-03T11:00:25.927000+00:00") {
                assert_eq!(meaning, "$SI:MAC. - $FN:....");
                assert_eq!(description, "file_path: \\.");
            } else {
                panic!("invalid date {timestamp}")
            }
        }
    }

    /// Tests parsing of an NTFSInfo CSV file with qualifiers and null handling.
    ///
    /// It ensures that:
    /// - The output JSONL file contains properly formatted qualified field names (e.g., "pe_subsystem:pe_subsystem")
    /// - The specified field is correctly identified as null in the second line
    /// - The file path in the second line matches expected value "\\$Bitmap"
    #[test]
    fn ntfs_info_qualifiers_and_null_parse() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo_qualifiers_null_parse";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.jsonl");

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

        let run_config = RunConfiguration::new(vec![output_conf], false, None);
        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());
        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );
        assert_eq!(report.last_error, None);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        //second line
        let line = lines[1].as_object().unwrap();
        let _subsystem = line
            .get("pe_subsystem:pe_subsystem")
            .unwrap()
            .as_str()
            .unwrap();

        let file_path = line.get("file_path:file_path").unwrap().as_str().unwrap();
        assert_eq!("\\$Bitmap", file_path)
    }

    /// Tests the parsing of an NTFSInfo CSV file with best-effort error handling.
    ///
    /// - When `log_before_fail` is 0, parsing fails immediately upon encountering an error
    /// - When `log_before_fail` is 1, one error is logged and parsing continues until a second error occurs
    /// - When `log_before_fail` is 2, two errors are logged and parsing continues
    #[test]
    fn ntfs_info_best_effort() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo_best_effort";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.jsonl");

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

        let run_config = RunConfiguration::new(vec![output_conf], false, None);

        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());
        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        // Fail immediately on first error
        let report = parse_csv(
            "test_data/csv/NTFSInfoError.csv",
            run_config.clone(),
            plugin_config.clone(),
            Metadata::new("test".into()),
            0,
        );

        match report.last_error {
            Some(e) => {
                assert!(
                    e.to_string()
                        .starts_with("An error occured while parsing column '6' in line: '2'")
                )
            }
            None => panic!("An error is expected"),
        }

        let jsonl = fs::read_to_string(&targetfile).unwrap();

        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(1, lines.len());
        fs::remove_file(&targetfile).unwrap();
        // Log one error before failing
        let report = parse_csv(
            "test_data/csv/NTFSInfoError.csv",
            run_config.clone(),
            plugin_config.clone(),
            Metadata::new("test".into()),
            1,
        );

        match report.last_error {
            Some(e) => {
                assert!(
                    e.to_string()
                        .starts_with("An error occured while parsing column '15' in line: '2'")
                )
            }
            None => panic!("An error is expected"),
        }

        let jsonl = fs::read_to_string(&targetfile).unwrap();

        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(1, lines.len());
        fs::remove_file(&targetfile).unwrap();
        // Log two errors and avoid failure
        let report = parse_csv(
            "test_data/csv/NTFSInfoError.csv",
            run_config.clone(),
            plugin_config.clone(),
            Metadata::new("test".into()),
            2,
        );
        match report.last_error {
            Some(e) => assert!(
                e.to_string()
                    .starts_with("An error occured while parsing column '6' in line: '4'")
            ),
            None => panic!("An error is expected"),
        }

        let jsonl = fs::read_to_string(&targetfile).unwrap();

        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(3, lines.len());
        fs::remove_file(&targetfile).unwrap();
    }

    /// Tests the parsing of an NTFSInfo CSV file with unmapped fields in the timeline.
    ///
    /// This test verifies that:
    /// - No errors are returned during parsing (i.e., `report.last_error` is `None`)
    /// - The correct number of lines are parsed (5)
    /// - The output JSONL file contains 5 lines
    /// - The unmapped field "size_in_bytes" is correctly included in the output with its value
    #[test]
    fn timeline_with_unmaped_filed() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo_timeline_with_unmapped";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.jsonl");

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
        let xml = fs::read_to_string("test_data/csv/NTFSInfo_with_unmapped.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        // Fail immediately on first error
        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            0,
        );
        assert_eq!(report.last_error, None);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(5, lines.len());

        let line = lines[3].as_object().unwrap();
        let addition_descr = line
            .get("additional_description")
            .unwrap()
            .as_str()
            .unwrap();

        assert_eq!(addition_descr, "size_in_bytes: 1950016")
    }
    /// Tests the parsing of an NTFSInfo CSV file.
    ///
    /// It ensures that:
    /// - No errors are returned during parsing (i.e., `report.last_error` is `None`)
    /// - The correct number of lines are parsed (3)
    /// - The output JSONL file contains 3 lines
    /// - Specific values from the first, second, and third lines are correctly extracted
    #[test]
    fn ntfs_info_csv_parse() {
        let output_folder = ".tmp";
        let base_file_name = "ntfsinfo";
        let targetfile = format!("{output_folder}/{base_file_name}.ntfs_info.csv");

        if Path::new(&targetfile).exists() {
            fs::remove_file(&targetfile).unwrap();
        }

        let output_conf = OutputConfiguration::new(
            base_file_name.to_string(),
            ".tmp".to_string(),
            "file".to_string(),
            "csv".to_string(),
            "iso".to_string(),
            false,
            false,
            false,
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], false, None);

        let xml = fs::read_to_string("test_data/csv/NTFSInfo.xml").unwrap();
        let mut parser_ext: HashMap<String, ParserExtension> = HashMap::new();
        parser_ext.insert("Attributes".to_string(), win_ntfs_flag_parser());
        parser_ext.insert("FRN".to_string(), win_frn_hex_parser(""));
        parser_ext.insert("SignedHash".to_string(), win_signed_hash_parser());

        let plugin_config = PluginConfiguration::from_str(&xml, None, Some(parser_ext)).unwrap();

        let report = parse_csv(
            "test_data/csv/NTFSInfo.csv",
            run_config,
            plugin_config,
            Metadata::new("test".into()),
            1000,
        );

        assert_eq!(report.last_error, None);
        let result = &report.output_reports[0];

        let expected_lines = 3;
        assert_eq!(expected_lines, result.file_reports[0].num_lines);

        let csv = fs::read_to_string(targetfile).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(csv.as_bytes()));
        let mut line_number = 0;

        for (line_nb, record) in reader.records().enumerate() {
            let rec = record.unwrap();

            assert_eq!(63, rec.len());

            assert_eq!(&rec[0], "{00000000-0000-0000-0000-000000000000}");
            match line_number {
                0 => assert_eq!(&rec[1], "\\."),
                1 => assert_eq!(&rec[1], "\\$Bitmap"),
                2 => assert_eq!(&rec[1], "\\test"),
                _ => panic!("unexpected line number: {line_number}"),
            }

            assert_eq!(
                &rec[62],
                "{\"computer\":\"test\",\"data_type\":\"ntfs_info\"}"
            );
            line_number = line_nb + 1;
        }
        assert_eq!(3, line_number);
    }

    #[test]
    fn autoruns_utf16() {
        let output_folder = ".tmp";
        let base_file_name = "autoruns";
        let targetfile = format!("{output_folder}/{base_file_name}.autoruns.jsonl");

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
        let xml = fs::read_to_string("test_data/csv/autoruns.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        // Fail immediately on first error
        let report = parse_csv(
            "test_data/csv/autoruns.csv",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
            0,
        );
        assert_eq!(report.last_error, None);

        let jsonl = fs::read_to_string(targetfile).unwrap();
        let lines: Vec<Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(99, lines.len());

        let line = lines[0].as_object().unwrap();
        let addition_descr = line
            .get("additional_description")
            .unwrap()
            .as_str()
            .unwrap();

        assert_eq!(
            addition_descr,
            "category: Boot Execute - profile: System-wide"
        )
    }
}
