use std::{collections::HashMap, path::Path};

use crate::{
    Error, FieldMapping, Metadata, Output, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration, windows_utils::convert_sid,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as enc64};
use chrono::{DateTime, Utc};
use libesedb::{EseDb, Table, systemtime_from_filetime, systemtime_from_oletime};
use log::warn;
use pyo3::prelude::*;

// Parses an SRUM (System Resource Usage Monitor) database file and returns a `RunReport`.
///
/// # Arguments
/// * `input_file` - Path to the SRUM database file.
/// * `configuration` - Configuration for the parsing run.
/// * `metadata` - Metadata associated with the parsing operation.
///
#[pyfunction]
pub fn parse_srum(
    input_file: &str,
    configuration: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport {
    match parse(input_file, configuration, plugin_config, metadata) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

/// Main parsing function for SRUM database files.
///
/// Initializes the `SrumParser` and processes all SRUM tables in the database.
///
/// # Arguments
/// * `input_file` - Path to the SRUM database file.
/// * `configuration` - Configuration for the parsing run.
/// * `metadata` - Metadata associated with the parsing operation.
///
pub fn parse(
    input_file: &str,
    configuration: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> Result<RunReport, Error> {
    let parser = SrumParser::new(input_file)?;
    let report = parser.parse_every_tables(configuration, plugin_config, metadata)?;
    Ok(report)
}
pub const ID_MAP_TABLE: &str = "SruDbIdMapTable";

/// A parser for SRUM (System Resource Usage Monitor) database files.
///
/// This struct provides methods to parse various SRUM tables and extract
/// structured data from the database.
pub struct SrumParser {
    db: EseDb,
    index: HashMap<i32, String>,
}
impl SrumParser {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let db = EseDb::open(path)?;
        let index = SrumParser::init_index(&db)?;

        Ok(SrumParser { db, index })
    }

    /// Initializes the index table used to map numeric IDs to human-readable values.
    ///
    /// This is used to resolve IDs in the SRUM database to their corresponding
    /// string representations (e.g., SIDs, application names, etc.).
    ///
    /// # Arguments
    /// * `db` - Reference to the SRUM database.
    ///
    fn init_index(db: &EseDb) -> Result<HashMap<i32, String>, Error> {
        let table = db.table_by_name(ID_MAP_TABLE)?;

        let num_records = table.count_records()? as usize;
        let mut index: HashMap<i32, String> = HashMap::with_capacity(num_records);

        for rec in table.iter_records()? {
            let rec = rec?;

            let id_type = match rec.value(0)?.to_u32() {
                Some(v) => v,
                None => continue,
            };

            let id_index = match rec.value(1)?.to_i32() {
                Some(v) => v,
                None => continue,
            };

            let blob = rec.value(2)?;
            let id_blob = match blob.as_bytes() {
                Some(v) => v,
                None => continue,
            };

            let value = if id_type == 3 {
                convert_sid(id_blob)?
            } else {
                String::from_utf8_lossy(id_blob).replace("\0", "")
            };

            index.insert(id_index, value);
        }
        Ok(index)
    }

    /// Parses all SRUM tables in the database.
    ///
    /// This method processes each SRUM table, maps columns to output fields,
    /// and writes the parsed data to the output.
    ///
    /// # Arguments
    /// * `configuration` - Configuration for the parsing run.
    /// * `metadata` - Metadata associated with the parsing operation.
    ///
    pub fn parse_every_tables(
        &self,
        configuration: RunConfiguration,
        plugin_config: PluginConfiguration,
        metadata: Metadata,
    ) -> Result<RunReport, Error> {
        let mut run_report = RunReport::new();

        for srum_table in srum_tables() {
            let table_name = &srum_table.name;
            let config = plugin_config.data_type_configs.iter().find(
                |config: &&crate::configuration::DataTypeMapping| config.data_type.eq(table_name),
            );
            let data_mapping = match config {
                Some(config) => config
                    .field_mapping
                    .clone()
                    .ok_or(Error::ConfigurationError(format!(
                        "'{table_name}' configuration doe not have a field mapping. Plugin: {}",
                        plugin_config.plugin
                    )))?,
                None => {
                    return Err(Error::ConfigurationError(format!(
                        "'{table_name}' not found in the plugin configuration. Plugin: {}",
                        plugin_config.plugin
                    )));
                }
            };

            let mut output = Output::new(
                configuration.clone(),
                plugin_config.clone(),
                metadata.clone(),
                Some(srum_table.name.to_owned()),
            )?;

            match self.db.table_by_name(srum_table.id) {
                Ok(table) => {
                    if let Err(e) = self.parse_table(&table, &mut output, &data_mapping) {
                        let error = format!("SRUM Table:'{}', Error: {e}", &srum_table.name,);
                        warn!("{error}");

                        let mut report = output.get_report();
                        report.last_error = Some(error);
                        report.num_errors += 1;
                        run_report.add_output_report(report);
                    } else {
                        run_report.add_output_report(output.get_report());
                    }
                }
                Err(e) => {
                    let error = format!("SRUM Table:'{}', Error: {e}", &srum_table.name,);
                    warn!("{error}");
                    //missing tables are expected, does not report the error
                }
            }
        }
        Ok(run_report)
    }

    /// Parses a single SRUM table and writes the results to the output.
    ///
    /// # Arguments
    /// * `table_name` - Name of the table to parse.
    /// * `output` - Output destination for the parsed data.
    /// * `output_map` - Mapping from column names to output field names.
    fn parse_table(
        &self,
        table: &Table,
        output: &mut Output,
        data_mapping: &FieldMapping,
    ) -> Result<(), Error> {
        //map columns
        let mut columns = Vec::new();
        for col in table.iter_columns()? {
            let col = col?;
            let col_name = col.name()?;
            let col_type = column_type(&col_name);

            columns.push((col_name.to_owned(), col_type));
        }

        for row in table.iter_records()? {
            let row = row?;
            let mut record = Record::new();

            for (pos, column) in row.iter_values()?.enumerate() {
                let (input_name, data_type) = &columns[pos];
                let column = column?;

                let value = match data_type {
                    SrumDataType::String => self.get_string_from_index(column),
                    SrumDataType::Date => {
                        let date = self.get_date(column);
                        if let Some(date) = date {
                            Value::Date(date)
                        } else {
                            Value::Null()
                        }
                    }
                    _ => parse_value(column),
                };
                data_mapping
                    .field_parser_tree
                    .set_field_value(input_name, value, &mut record)?;
                // tuple.add(input_name, value);
            }
            output.write(&mut record)?;
        }

        Ok(())
    }

    /// Retrieves a string value from the index map.
    ///
    /// # Arguments
    /// * `column` - The column value containing an ID.
    ///
    fn get_string_from_index(&self, column: libesedb::Value) -> Value {
        let index = column.to_i32();
        match index {
            Some(e) => match self.index.get(&e) {
                Some(s) => Value::String(s.clone()),
                None => Value::Null(),
            },
            None => Value::Null(),
        }
    }

    /// Converts internal date formats to UTC datetime.
    ///
    /// # Arguments
    /// * `column` - The column value containing a date.
    fn get_date(&self, column: libesedb::Value) -> Option<DateTime<Utc>> {
        match column {
            libesedb::Value::F64(e) => {
                let systime = systemtime_from_oletime(e);
                Some(systime.into())
            }
            libesedb::Value::DateTime(_) => column.to_oletime().map(|systime| systime.into()),
            libesedb::Value::I64(e) => {
                let systime = systemtime_from_filetime(e as u64);
                Some(systime.into())
            }

            _ => None,
        }
    }
}

///
/// convert data to json
///
fn parse_value(column: libesedb::Value) -> Value {
    match column {
        libesedb::Value::Bool(v) => Value::Bool(v),
        libesedb::Value::U8(v) => Value::Int(v as i64),
        libesedb::Value::I16(v) => Value::Int(v as i64),
        libesedb::Value::I32(v) => Value::Int(v as i64),
        libesedb::Value::F32(v) => Value::Float(v as f64),
        libesedb::Value::F64(v) => Value::Float(v),
        libesedb::Value::DateTime(v) => Value::Int(v as i64),
        libesedb::Value::U32(v) => Value::Int(v as i64),
        libesedb::Value::U16(v) => Value::Int(v as i64),
        libesedb::Value::Text(v) | libesedb::Value::LargeText(v) => Value::String(v),
        libesedb::Value::I64(v) | libesedb::Value::Currency(v) => Value::Int(v),
        libesedb::Value::Binary(items)
        | libesedb::Value::LargeBinary(items)
        | libesedb::Value::Guid(items)
        | libesedb::Value::SuperLarge(items) => Value::String(enc64.encode(&items)),
        _ => Value::Null(),
    }
}

///
/// Data types used in the field definition
///
enum SrumDataType {
    String,
    Date,
    Data,
}

///
/// Definition of a srum table, with its topic name used by the ouput writers
/// https://github.com/libyal/esedb-kb/blob/main/documentation/System%20Resource%20Usage%20Monitor%20(SRUM).asciidoc
///
pub struct SrumTable {
    pub name: &'static str,
    pub id: &'static str,
}

pub fn srum_tables() -> Vec<SrumTable> {
    vec![
        SrumTable {
            name: "srum_app_timeline",
            id: "{5C8CF1C7-7257-4F13-B223-970EF5939312}",
        },
        SrumTable {
            name: "srum_application_resources",
            id: "{D10CA2FE-6FCF-4F6D-848E-B2E99266FA89}",
        },
        SrumTable {
            name: "srum_energy_estimation",
            id: "{DA73FB89-2BEA-4DDC-86B8-6E048C6DA477}",
        },
        SrumTable {
            name: "srum_energy_usage",
            id: "{FEE4E14F-02A9-4550-B5CE-5FA2DA202E37}",
        },
        SrumTable {
            name: "srum_energy_usage_long_term",
            id: "{FEE4E14F-02A9-4550-B5CE-5FA2DA202E37}LT",
        },
        SrumTable {
            name: "srum_network_connectivity_usage",
            id: "{DD6636C4-8929-4683-974E-22C046A43763}",
        },
        SrumTable {
            name: "srum_network_data_usage",
            id: "{973F5D5C-1D90-4944-BE8E-24B94231A174}",
        },
        //sdp tables referenced here https://github.com/EricZimmerman/Srum/issues/15
        SrumTable {
            name: "srum_sdp_volume",
            id: "{17F4D97B-F26A-5E79-3A82-90040A47D13D}",
        },
        SrumTable {
            name: "srum_sdp_physical_disk",
            id: "{841A7317-3805-518B-C2EA-AD224CB4AF84}",
        },
        SrumTable {
            name: "srum_sdp_cpu",
            id: "{DC3D3B50-BB90-5066-FA4E-A5F90DD8B677}",
        },
        SrumTable {
            name: "srum_sdp_network",
            id: "{EEE2F477-0659-5C47-EF03-6D6BEFD441B3}",
        },
        SrumTable {
            name: "srum_tagged_energy",
            id: "{B6D82AF1-F780-4E17-8077-6CB9AD8A6FC4}",
        },
        SrumTable {
            name: "srum_vfuprov",
            id: "{7ACBBAA3-D029-4BE4-9A7A-0885927F1D8F}",
        },
        SrumTable {
            name: "srum_wpn_provider",
            id: "{D10CA2FE-6FCF-4F6D-848E-B2E99266FA86}",
        },
    ]
}

///
/// retrieve the field type for a column
///
fn column_type(column_name: &str) -> SrumDataType {
    match column_name {
        "TimeStamp" => SrumDataType::Date,
        "AppId" => SrumDataType::String,
        "UserId" => SrumDataType::String,
        "ConnectStartTime" => SrumDataType::Date,
        _ => SrumDataType::Data,
    }
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, fs};

    use crate::OutputConfiguration;

    use super::*;

    #[test]
    fn test_srum() {
        let output_folder = ".tmp";
        let base_file_name = "srum_test";
        let targetfile = format!("{output_folder}/{base_file_name}");

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
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);
        let xml = fs::read_to_string("test_data/srum/srum.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_srum(
            "test_data/srum/SRUDB.dat",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
        );

        if let Some(e) = report.last_error {
            panic!("{e}");
        }

        let mut num_rows = 0;
        for orep in report.output_reports {
            for file in orep.file_reports {
                num_rows += file.num_lines
            }
        }
        assert_eq!(num_rows, 2355);
    }

    #[test]
    fn test_srum_timeline() {
        let output_folder = ".tmp";
        let base_file_name = "srum_timeline";
        let targetfile = format!("{output_folder}/{base_file_name}");

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
            HashMap::new(),
        );

        let run_config = RunConfiguration::new(vec![output_conf], true, None);

        let xml = fs::read_to_string("test_data/srum/srum.xml").unwrap();
        let plugin_config = PluginConfiguration::from_str(&xml, None, None).unwrap();

        let report = parse_srum(
            "test_data/srum/SRUDB.dat",
            run_config.clone(),
            plugin_config,
            Metadata::new("test".into()),
        );

        let mut num_rows = 0;
        for orep in report.output_reports {
            for file in orep.file_reports {
                num_rows += file.num_lines
            }
        }
        assert_eq!(num_rows, 2361);
    }
}
