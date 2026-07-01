use pyo3::prelude::*;

use crate::{
    FileReport, Metadata, MultiInputField, Record, RunReport, Value,
    config_parser::{OutputConfiguration, PluginDescription, RunConfiguration},
    configuration::{DataTypeMapping, PluginConfiguration},
    date_util::{DateInputCodec, DateOutputCodec},
    field::{ArrayField, Field, FieldName, MultiParser, Parser, ParserExtension, PyParser},
    field_mapping::{FieldMapping, FieldParser, FieldParserTree, ParserType},
    output::{COMPRESSION_LEVEL, Output},
    parse_csv, parse_hive_keys, parse_regexp, parse_sqlite, parse_srum,
    parser::{
        evtx::parse_evtx,
        hive::parse_hive_values,
        json::{parse_json, parse_jsonl},
        windows_parsers::{
            win_frn_hex_parser, win_frn_int_parser, win_ntfs_flag_parser, win_signed_hash_parser,
        },
        xml::parse_xml,
    },
    registry_api::{RegKey, RegValue, Registry},
    seven_zip_unpack::{FilesToExtract, extract_7z_file, extract_7z_files},
    timeline::{TimeLineBuilder, TimeLineType, TimelineDisplayOptions},
    windows_utils::{SecurityDescriptor, SecurityDescriptorAce, security_descriptor_from_bytes},
};

/// A minimal base class for plugins in the anssi-ogre-common project,
/// allowing Python users to subclass and implement custom behavior (e.g., parse() logic).
#[derive(Default)]
#[pyclass(subclass)]
pub struct OgrePlugin {}
#[pymethods]
impl OgrePlugin {
    #[new]
    pub fn new() -> Self {
        Self {}
    }
}

/// A minimal base class for plugins that process batch of small files,
/// allowing Python users to subclass and implement custom behavior (e.g., parse() logic).
#[derive(Default)]
#[pyclass(subclass)]
pub struct OgreBatchedPlugin {}
#[pymethods]
impl OgreBatchedPlugin {
    #[new]
    pub fn new() -> Self {
        Self {}
    }
}
/// a batch entry to be used by python OgreBatchedPlugin
#[derive(Default, Clone)]
#[pyclass(get_all, set_all, from_py_object)]
pub struct BatchEntry {
    file: String,
    run_config: RunConfiguration,
    metadata: Metadata,
}
#[pymethods]
impl BatchEntry {
    #[new]
    pub fn new(file: String, run_config: RunConfiguration, metadata: Metadata) -> Self {
        Self {
            file,
            run_config,
            metadata,
        }
    }
}

/// A minimal base class to create custom field parser in python,
#[derive(Default)]
#[pyclass(subclass)]
pub struct AbstractParser {}
#[pymethods]
impl AbstractParser {
    #[new]
    pub fn new() -> Self {
        Self {}
    }
}

/// A Python module implemented in Rust.
#[pymodule]
pub fn dfir_ogre_common(m: &Bound<'_, PyModule>) -> PyResult<()> {
    pyo3_log::init();
    m.add_class::<AbstractParser>()?;
    m.add_class::<ArrayField>()?;
    m.add_class::<BatchEntry>()?;
    m.add_class::<DateInputCodec>()?;
    m.add_class::<DateOutputCodec>()?;
    m.add_class::<Field>()?;
    m.add_class::<FieldName>()?;
    m.add_class::<FieldMapping>()?;
    m.add_class::<FieldParser>()?;
    m.add_class::<FieldParserTree>()?;
    m.add_class::<ParserType>()?;
    m.add_class::<RegKey>()?;
    m.add_class::<RegValue>()?;
    m.add_class::<Registry>()?;
    m.add_class::<Metadata>()?;
    m.add_class::<MultiInputField>()?;
    m.add_class::<MultiParser>()?;
    m.add_class::<OgreBatchedPlugin>()?;
    m.add_class::<OgrePlugin>()?;
    m.add_class::<Output>()?;
    m.add_class::<OutputConfiguration>()?;
    m.add_class::<FileReport>()?;
    m.add_class::<FilesToExtract>()?;
    m.add_class::<Parser>()?;
    m.add_class::<ParserExtension>()?;
    m.add_class::<PluginConfiguration>()?;
    m.add_class::<DataTypeMapping>()?;
    m.add_class::<PluginDescription>()?;
    m.add_class::<PyParser>()?;
    m.add_class::<SecurityDescriptor>()?;
    m.add_class::<SecurityDescriptorAce>()?;
    m.add_class::<RunConfiguration>()?;
    m.add_class::<RunReport>()?;
    m.add_class::<TimeLineType>()?;
    m.add_class::<TimelineDisplayOptions>()?;
    m.add_class::<TimeLineBuilder>()?;
    m.add_class::<Record>()?;
    m.add_class::<Value>()?;
    m.add_function(wrap_pyfunction!(extract_7z_file, m)?)?;
    m.add_function(wrap_pyfunction!(extract_7z_files, m)?)?;
    m.add_function(wrap_pyfunction!(parse_csv, m)?)?;
    m.add_function(wrap_pyfunction!(parse_evtx, m)?)?;
    m.add_function(wrap_pyfunction!(parse_hive_keys, m)?)?;
    m.add_function(wrap_pyfunction!(parse_json, m)?)?;
    m.add_function(wrap_pyfunction!(parse_jsonl, m)?)?;
    m.add_function(wrap_pyfunction!(parse_hive_values, m)?)?;
    m.add_function(wrap_pyfunction!(parse_regexp, m)?)?;
    m.add_function(wrap_pyfunction!(parse_sqlite, m)?)?;
    m.add_function(wrap_pyfunction!(parse_srum, m)?)?;
    m.add_function(wrap_pyfunction!(parse_xml, m)?)?;
    m.add_function(wrap_pyfunction!(security_descriptor_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(win_frn_hex_parser, m)?)?;
    m.add_function(wrap_pyfunction!(win_frn_int_parser, m)?)?;
    m.add_function(wrap_pyfunction!(win_ntfs_flag_parser, m)?)?;
    m.add_function(wrap_pyfunction!(win_signed_hash_parser, m)?)?;

    m.add("COMPRESSION_LEVEL", COMPRESSION_LEVEL)?;
    Ok(())
}
