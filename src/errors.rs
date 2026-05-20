use thiserror::Error;

use pyo3::{exceptions, prelude::*};

/// convert thiserror into a python error
impl std::convert::From<Error> for PyErr {
    fn from(err: Error) -> PyErr {
        exceptions::PyException::new_err(err.to_string())
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Csv(#[from] csv::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    ParseBool(#[from] std::str::ParseBoolError),

    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),

    #[error(transparent)]
    ParseFloat(#[from] std::num::ParseFloatError),

    #[error(transparent)]
    ParseDate(#[from] chrono::ParseError),

    #[error(transparent)]
    Pyo3(#[from] pyo3::PyErr),

    #[error(transparent)]
    Regex(#[from] regex::Error),

    #[error(transparent)]
    Evtx(#[from] evtx::err::EvtxError),

    #[error(transparent)]
    NtHive(#[from] dfir_nt_hive::NtHiveError),

    #[error(transparent)]
    SevenZip(#[from] sevenz_rust2::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    XMLTree(#[from] xmltree::ParseError),

    #[error(transparent)]
    XpathParser(#[from] sxd_xpath::ParserError),

    #[error(transparent)]
    XpathExecution(#[from] sxd_xpath::ExecutionError),

    #[error(transparent)]
    XmlDocument(#[from] sxd_document::parser::Error),

    #[error("'{0}' is not an object")]
    JsonNotAnObject(String),

    #[error("{0}")]
    EvtxParsing(String),

    #[error("while parsing field: '{0}', Error:'{1}'")]
    ParseField(String, String),

    #[error("While Reading File: '{0}' Error: {1}")]
    FileRead(String, String),

    #[error("Unknown Qualifier: '{0}'")]
    UnknownQualifier(String),

    #[error("Field '{0}' does not have an associated python parser")]
    UnknownPythonParser(String),

    #[error("Field '{0}' does not have an associated parser extension")]
    UnknownParserExtension(String),

    #[error("Could not retrieve DataTypeMapping for name {0}")]
    UnknownDataTypeMapping(String),

    #[error("Field '{0}' array of arrays is not supported")]
    UnsupportedNestingArray(String),

    #[error("{0} is not supported")]
    InvalidDataType(String),

    #[error("Invalid timestamp")]
    InvalidTimestamp(),

    #[error("field: {0} is not a time field")]
    InvalidTimeField(String),

    #[error("JSON value is not a String: '{0}'")]
    JsonNotAString(String),

    #[error("'{0}' is not an allowed output type")]
    InvalidOutputType(String),

    #[error("'{0}' is not an allowed output format")]
    InvalidOutputFormat(String),

    #[error("'{0}' parameter is missing")]
    MissingParameter(String),

    #[error("No DateTimeOutputCodec was provided")]
    MissingDateTimeCodec(),

    #[error("Cannot build timeline: No TimelineBuilder provided")]
    MissingTimeLineBuilder(),

    #[error("Cannot build timeline: No Dates in the line ")]
    MissingTimeLineBuilderDates(),

    #[error("{0} Not an object ")]
    InvalidObject(String),

    #[error("{0}")]
    PluginError(String),

    #[error("An error occured while parsing line: '{0}', error: {1}")]
    ParsingLine(usize, String),

    #[error("An error occured while parsing column '{2}' in line: '{0}', error: {1}")]
    ParsingColumn(usize, String, usize),

    #[error("At line:'{0}' Log does not match regex:'{1}' ")]
    InvalidLogRegex(usize, String),

    #[error("'{0}' cannot be empty")]
    CannotBeEmpty(String),

    #[error("For field '{0}' set value is not an Object")]
    ValueNotObject(String),

    #[error("For field '{0}' set value is not supported for MultiInputField")]
    ValueNotSupportedForMultiInputField(String),

    #[error("For field '{0}' set value requires a '{1}' value")]
    ValueTypeInvalid(String, String),

    #[error(
        "For field: '{0}' FieldParserTree can only directly parse flat field, not Object or array fields"
    )]
    InvalidParserType(String),

    #[error("'{0}'")]
    EvtxError(String),

    #[error("'{0}'")]
    NtHiveError(String),

    #[error("'{0}'")]
    ConfigurationError(String),

    #[error("Failed to parse '{0}': invalid byte lenght")]
    InvalidByteLenght(String),

    #[error("Failed to parse {0} value from number for field '{1}'")]
    FieldParserError(String, String),
}
