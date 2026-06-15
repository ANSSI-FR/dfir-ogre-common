use base64::{Engine as _, engine::general_purpose::URL_SAFE as BASE64};
use std::{
    collections::{HashMap, HashSet},
    ops::RangeFull,
    sync::{Arc, LazyLock, Mutex},
};

use blake3::Hasher;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use pyo3::prelude::*;

use crate::{
    FieldName, Record, Value, date_util::DateOutputCodec, errors::Error, escape_special_chars,
    format_csv::CSV_DELIMITER, metadata::MetadataSerialized, serialize_date,
    value::escape_special_chars_csv,
};

const MACB_FILLER: &str = "MACBMACB";

/// Holds the MACB lookup table for timestamp metadata.
static MACB_TABLE: LazyLock<HashMap<String, u8>> = LazyLock::new(|| {
    let mut m = std::collections::HashMap::new();
    m.insert("si_lastmod_date".to_string(), 0b10000000);
    m.insert("si_lastaccess_date".to_string(), 0b01000000);
    m.insert("si_lastchange_date".to_string(), 0b00100000);
    m.insert("si_creation_date".to_string(), 0b00010000);
    m.insert("fn_lastmod_date".to_string(), 0b00001000);
    m.insert("fn_lastaccess_date".to_string(), 0b00000100);
    m.insert("fn_lastchange_date".to_string(), 0b00000010);
    m.insert("fn_creation_date".to_string(), 0b00000001);
    m
});
#[derive(Debug, Clone, Default)]
pub struct TimeLine {
    pub computer: Option<String>,
    pub id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub timestamp_meaning: String,
    pub data_type: String,
    pub related_user: String,
    pub description: String,
    pub additional_description: String,
    pub metadata_id: Option<String>,
    pub metadata: Option<MetadataSerialized>,
    pub data_id: Option<String>,
    pub data: Option<Record>,
}
impl TimeLine {
    pub fn json_serialise(
        &self,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
        include_empty: bool,
    ) -> Result<(), Error> {
        buffer.push('{');
        let mut first = true;
        if let Some(id) = &self.id {
            Self::serialise_json_string("id", id, buffer, first);
            first = false
        }
        if let Some(computer) = &self.computer {
            Self::serialise_json_string("computer", computer, buffer, first);
            first = false
        }

        Self::serialise_json_date("timestamp", &self.timestamp, date_codec, buffer, first);
        first = false;
        Self::serialise_json_string("timestamp_meaning", &self.timestamp_meaning, buffer, first);
        Self::serialise_json_string("data_type", &self.data_type, buffer, first);
        Self::serialise_json_string("related_user", &self.related_user, buffer, first);
        Self::serialise_json_string("description", &self.description, buffer, first);
        Self::serialise_json_string(
            "additional_description",
            &self.additional_description,
            buffer,
            first,
        );
        if let Some(metadata_id) = &self.metadata_id {
            Self::serialise_json_string("metadata_id", metadata_id, buffer, first);
        }
        if let Some(metadata) = &self.metadata {
            buffer.push(',');
            buffer.push_str("\"metadata\":");
            buffer.push_str(metadata.0.as_str());
        }
        if let Some(data_id) = &self.data_id {
            Self::serialise_json_string("data_id", data_id, buffer, first);
        }
        if let Some(data) = &self.data {
            buffer.push(',');
            buffer.push_str("\"data\":");
            data.json_serialise(buffer, date_codec, include_empty)?;
        }
        buffer.push('}');
        Ok(())
    }

    pub fn compute_id(&mut self, hasher: &mut Hasher) {
        hasher.update("computer".as_bytes());
        if let Some(computer) = &self.computer {
            hasher.update(computer.as_bytes());
        }
        hasher.update("timestamp".as_bytes());
        if let Some(nanos) = self.timestamp.timestamp_nanos_opt() {
            hasher.update(&nanos.to_ne_bytes());
        } else {
            // Hash a zero value if the timestamp is missing or out‑of‑range.
            hasher.update(&0i64.to_ne_bytes());
        }
        hasher.update("timestamp_meaning".as_bytes());
        hasher.update(self.timestamp_meaning.as_bytes());
        hasher.update("data_type".as_bytes());
        hasher.update(self.data_type.as_bytes());

        if let Some(data_id) = &self.data_id {
            hasher.update(data_id.as_bytes());
        }

        self.id = Some(BASE64.encode(hasher.finalize().as_slice()));
    }

    fn serialise_json_string(name: &str, value: &str, buffer: &mut String, first: bool) {
        if !first {
            buffer.push(',');
        }
        buffer.push('\"');
        buffer.push_str(name);
        buffer.push_str("\":");
        buffer.push('\"');
        buffer.push_str(&escape_special_chars(value));
        buffer.push('\"');
    }

    fn serialise_json_date(
        name: &str,
        date: &DateTime<Utc>,
        date_codec: &DateOutputCodec,
        buffer: &mut String,
        first: bool,
    ) {
        if !first {
            buffer.push(',');
        }
        buffer.push('\"');
        buffer.push_str(name);
        buffer.push_str("\":");
        buffer.push('\"');
        buffer.push_str(&serialize_date(date, date_codec));
        buffer.push('\"');
    }

    pub fn csv_serialise(
        &self,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
        include_empty: bool,
        normalized: bool,
    ) -> Result<(), Error> {
        let mut first = true;
        if normalized {
            if let Some(id) = &self.id {
                Self::serialise_csv_string(id, buffer, first);
            } else {
                Self::serialise_csv_string("", buffer, first);
            }
            first = false;
        }
        if let Some(computer) = &self.computer {
            Self::serialise_csv_string(computer, buffer, first);
        } else {
            Self::serialise_csv_string("", buffer, first);
        }
        first = false;

        Self::serialise_csv_date(&self.timestamp, date_codec, buffer, first);
        Self::serialise_csv_string(&self.timestamp_meaning, buffer, first);
        Self::serialise_csv_string(&self.data_type, buffer, first);
        Self::serialise_csv_string(&self.related_user, buffer, first);
        Self::serialise_csv_string(&self.description, buffer, first);
        Self::serialise_csv_string(&self.additional_description, buffer, first);

        if normalized {
            if let Some(metadata_id) = &self.metadata_id {
                Self::serialise_csv_string(metadata_id, buffer, first);
            } else {
                Self::serialise_csv_string("", buffer, first);
            }
            if let Some(data_id) = &self.data_id {
                Self::serialise_csv_string(data_id, buffer, first);
            } else {
                Self::serialise_csv_string("", buffer, first);
            }
        } else {
            if let Some(metadata) = &self.metadata {
                buffer.push(CSV_DELIMITER);
                buffer.push('\"');
                buffer.push_str(metadata.0.as_str());
                buffer.push('\"');
            } else {
                Self::serialise_csv_string("", buffer, first);
            }
            if let Some(data) = &self.data {
                buffer.push(CSV_DELIMITER);
                buffer.push('\"');
                let mut object_buffer = String::new();
                data.json_serialise(&mut object_buffer, date_codec, include_empty)?;
                let escaped = escape_special_chars_csv(&object_buffer);
                buffer.push_str(&escaped);
                buffer.push('\"');
            } else {
                Self::serialise_csv_string("", buffer, first);
            }
        }
        Ok(())
    }

    fn serialise_csv_string(value: &str, buffer: &mut String, first: bool) {
        if !first {
            buffer.push(CSV_DELIMITER)
        }
        buffer.push('\"');
        buffer.push_str(&escape_special_chars_csv(value));
        buffer.push('\"');
    }

    pub fn csv_serialise_header(buffer: &mut String, normalized: bool) {
        if normalized {
            buffer.push_str("id");
            buffer.push(CSV_DELIMITER);
        }
        buffer.push_str("computer");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("timestamp");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("timestamp_meaning");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("data_type");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("related_user");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("description");
        buffer.push(CSV_DELIMITER);
        buffer.push_str("additional_description");
        buffer.push(CSV_DELIMITER);
        if normalized {
            buffer.push_str("metadata_id");
        } else {
            buffer.push_str("metadata");
        }
        buffer.push(CSV_DELIMITER);
        if normalized {
            buffer.push_str("data_id");
        } else {
            buffer.push_str("data");
        }
    }

    fn serialise_csv_date(
        date: &DateTime<Utc>,
        date_codec: &DateOutputCodec,
        buffer: &mut String,
        first: bool,
    ) {
        if !first {
            buffer.push(CSV_DELIMITER)
        }
        buffer.push_str(&serialize_date(date, date_codec));
    }
}

///
/// Different ways a timeline can be formatted. Pick the one that matches your data source.
///
/// * **MacbMacb** – MACB metadata appears twice, once for the `si_` fields and once for the `fn_` fields.
/// * **Macb** – The classic MACB layout (Modified, Accessed, Changed, Birth) using a single byte of flags.
/// * **Standard** – Plain timestamps without any MACB encoding.
///
#[derive(Debug, Clone, Default)]
#[pyclass(from_py_object)]
pub enum TimeLineType {
    MacbMacb,
    #[default]
    Standard,
}

/// Controls whether we prepend the field name and what separator we use when building description strings.
#[derive(Debug, Clone)]
#[pyclass(get_all, from_py_object)]
pub struct TimelineDisplayOptions {
    pub include_field_name: bool,
    pub field_separator: String,
}
#[pymethods]
impl TimelineDisplayOptions {
    #[new]
    pub fn new(include_field_name: bool, field_separator: String) -> Self {
        Self {
            include_field_name,
            field_separator,
        }
    }
}
impl Default for TimelineDisplayOptions {
    fn default() -> Self {
        Self {
            include_field_name: true,
            field_separator: " - ".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
#[pyclass(from_py_object)]
pub struct ConditionalDescriptionConf {
    conditions: Vec<(Vec<String>, Value)>,
    optional_field: Vec<Vec<String>>,
}
impl ConditionalDescriptionConf {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn add_condition(&mut self, path: Vec<String>, value: Value) {
        self.conditions.push((path, value));
    }

    pub fn add_optional_field(&mut self, path: Vec<String>) {
        self.optional_field.push(path);
    }
}

#[derive(Debug, Clone)]
pub struct ConditionalDescription {
    conditions: HashMap<String, Value>,
    optional_fields: HashSet<String>,
    checked_conditions: HashSet<String>,
    fields: IndexMap<String, Value>,
}
impl ConditionalDescription {
    pub fn new(conditions: HashMap<String, Value>, optional_fields: HashSet<String>) -> Self {
        let condition_len = conditions.len();
        let optional_fields_len = optional_fields.len();
        Self {
            conditions,
            optional_fields,
            checked_conditions: HashSet::with_capacity(condition_len),
            fields: IndexMap::with_capacity(optional_fields_len),
        }
    }

    pub fn evaluate(&mut self, name: &str, value: &Value) {
        if let Some(expected) = self.conditions.get(name)
            && value == expected
        {
            self.checked_conditions.insert(name.to_string());
        }
        if self.optional_fields.contains(name) {
            self.fields.insert(name.to_string(), value.clone());
        }
    }

    pub fn drain_values(&mut self) -> Option<Vec<(String, Value)>> {
        let values = if self.checked_conditions.len() == self.conditions.len() {
            let s: Vec<(String, Value)> = self.fields.drain(RangeFull).collect();
            Some(s)
        } else {
            None
        };
        self.checked_conditions.clear();
        self.fields.clear();
        values
    }

    pub fn clear(&mut self) {
        self.checked_conditions.clear();
        self.fields.clear();
    }
}

#[derive(Debug, Clone)]
#[pyclass(from_py_object)]
pub struct ConditionalDescritionField(pub Arc<Mutex<ConditionalDescription>>);

/// The kinds of data you might see in a timeline entry.
///
/// Groups fields like user info or textual descriptions so the builder knows how to handle them.
///
/// * **RelatedUser** – Holds data about who triggered the event.
/// * **DescriptionField** – The main text describing what happened.
/// * **AdditionalDescriptionField** – Extra details that go beyond the main description.
///
#[derive(Debug, Clone)]
pub enum TimelineFieldType {
    RelatedUser,
    DescriptionField,
    AdditionalDescriptionField,
    ConditionalField {
        conditional: ConditionalDescritionField,
    },
    OtherwiseField,
}
/// How a single piece of the timeline is stored.
/// * **Field** – a flat list of `TimelineFieldType`s (user, description, …).
/// * **Map** – a nested dictionary of more fields, letting you build trees of data.
///
#[derive(Debug, Clone, Default)]
pub struct TimelineField {
    pub fields: Vec<TimelineFieldType>,
    pub nested: HashMap<String, TimelineField>,
}
impl TimelineField {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
}

/// Tracks why a timestamp exists – either by MACB bits or by named fields.
///
/// This struct is used to record the origin of each timestamp in a timeline, supporting multiple timeline formats.
/// When we’re in `MacbMacb` mode we pack the four MACB bits into a single byte – the high nibble for `si_` fields, the low nibble for `fn_` fields.
///
/// Otherwise we simply keep a list of readable field names that produced the timestamp.
///
#[derive(Debug, Clone, Default)]
pub struct TimestampMeaning {
    timeline_type: TimeLineType,
    meaning_of_timestamp: HashSet<String>,
    max_date_meaning: usize,
    macbmacb: u8,
}

impl TimestampMeaning {
    /// # Arguments
    ///
    /// - `timeline_type`: The format of the timeline (e.g., `MacbMacb`, `Macb`, `Standard`).
    /// - `max_date_meaning`: The maximum number of distinct meanings a timestamp can have. If set to 0, no limit is enforced.
    ///
    pub fn new(timeline_type: TimeLineType, max_date_meaning: usize) -> Self {
        Self {
            timeline_type,
            max_date_meaning,
            ..Default::default()
        }
    }

    /// Adds a field name to the meaning of a timestamp, either by encoding it in MACB bits
    /// or by appending the field's description to the list of meanings.
    ///
    /// In `MacbMacb` mode, this method looks up the field name in `MACB_TABLE` and sets the corresponding bit.
    /// If the field is not found in the table, it falls back to storing the field's description.
    ///
    /// In other modes, it appends the field description to `meaning_of_timestamp`, provided the limit hasn't been exceeded.
    ///
    /// # Arguments
    ///
    /// - `fied_name`: A reference to the `FieldName` that generated the timestamp.
    ///
    pub fn add_meaning(&mut self, fied_name: &FieldName) {
        if let TimeLineType::MacbMacb = self.timeline_type {
            if let Some(&macb) = MACB_TABLE.get(fied_name.output_name()) {
                self.macbmacb |= macb;
            } else if self.meaning_of_timestamp.len() < self.max_date_meaning
                || self.max_date_meaning == 0
            {
                self.meaning_of_timestamp
                    .insert(fied_name.display().to_owned());
            }
        } else if self.meaning_of_timestamp.len() < self.max_date_meaning
            || self.max_date_meaning == 0
        {
            self.meaning_of_timestamp
                .insert(fied_name.display().to_owned());
        }
    }

    /// Converts the `TimestampMeaning` into a human-readable string representation.
    ///
    /// If `macbmacb` is non-zero, it generates a MACB-style string formatted as `$SI:XXXX - $FN:XXXX`, where `X` is either `M`, `A`, `C`, or `B`
    /// (from `MACBMACB`), or `.` if the bit is unset.
    ///
    /// Any additional field names (from `meaning_of_timestamp`) are appended to the result, separated by `" - "`.
    ///
    pub fn format_meaning(&self) -> String {
        let mut meaning = vec![];
        if self.macbmacb != 0 {
            let binary = format!("{:08b}", self.macbmacb);
            let mut macb_desc = Vec::new();
            for (i, c) in binary.chars().enumerate() {
                if c == '0' {
                    macb_desc.push('.');
                } else {
                    macb_desc.push(MACB_FILLER.chars().nth(i).unwrap_or('.'));
                }
            }

            let si_part = format!["$SI:{}", &macb_desc[0..4].iter().collect::<String>()];
            let fn_part = format!["$FN:{}", &macb_desc[4..8].iter().collect::<String>()];
            meaning.push(si_part);
            meaning.push(fn_part);
        }

        let mut sorted_meaning: Vec<String> = self
            .meaning_of_timestamp
            .iter()
            .map(|s| s.to_owned())
            .collect();
        sorted_meaning.sort();

        meaning.append(&mut sorted_meaning);
        meaning.join(" - ")
    }
}

#[derive(Debug, Clone, Default)]
pub struct TimelineData {
    pub related_user_field: Vec<Value>,
    pub description_field: IndexMap<String, Vec<Value>>,
    pub additional_description_field: IndexMap<String, Vec<Value>>,
    pub otherwise_field: IndexMap<String, Vec<Value>>,
    pub timestamps: HashMap<DateTime<Utc>, TimestampMeaning>,
}
impl TimelineData {
    pub fn clear(&mut self) {
        self.related_user_field.clear();
        self.description_field.clear();
        self.additional_description_field.clear();
        self.otherwise_field.clear();
        self.timestamps.clear();
    }
}

#[derive(Debug, Clone, Default)]
pub struct TimelineDataField(Arc<Mutex<TimelineData>>);
impl TimelineDataField {
    pub fn new(timeline_data: TimelineData) -> Self {
        Self(Arc::new(Mutex::new(timeline_data)))
    }
    pub fn clear(&self) {
        let mut data = self.0.lock().expect("mutex lock panicked");
        data.clear()
    }
}

/// The `TimeLineBuilder` allows for the incremental construction of a timeline
///
#[derive(Debug, Clone, Default)]
#[pyclass(from_py_object)]
pub struct TimeLineBuilder {
    pub timeline_type: TimeLineType,
    pub data_type: String,
    pub fields: HashMap<String, TimelineField>,
    pub conditional_fields: Vec<ConditionalDescritionField>,
    pub description_format: TimelineDisplayOptions,
    pub additional_description_format: TimelineDisplayOptions,
    pub max_date_meaning: usize,
    pub timeline_data: TimelineDataField,
}

#[pymethods]
impl TimeLineBuilder {
    /// Creates a new `TimeLineBuilder` instance with the provided configuration.
    ///
    /// # Arguments
    ///
    /// - `timeline_type`: The timeline format to use (e.g., `MacbMacb`, `Macb`, `Standard`).
    /// - `time_zone`: The time zone of the timeline data (e.g., "UTC").
    /// - `source_type`: The source system type (e.g., "File").
    /// - `data_type`: The data type or category (e.g., "Evtx, NtfsInfo, etc.").
    /// - `max_date_meaning`: The maximum number of field meanings allowed per timestamp (default: 0, no limit).
    ///
    #[new]
    #[pyo3(signature = (timeline_type, data_type, max_date_meaning=0, description_format=None,additional_description_format=None ))]
    pub fn new(
        timeline_type: TimeLineType,
        data_type: String,
        max_date_meaning: usize,
        description_format: Option<TimelineDisplayOptions>,
        additional_description_format: Option<TimelineDisplayOptions>,
    ) -> Self {
        TimeLineBuilder {
            timeline_type,
            data_type,
            fields: HashMap::new(),
            max_date_meaning,
            description_format: description_format.unwrap_or_default(),
            additional_description_format: additional_description_format.unwrap_or_default(),
            conditional_fields: Vec::new(),
            timeline_data: TimelineDataField::new(TimelineData {
                ..Default::default()
            }),
        }
    }

    pub fn clear(&mut self) {
        self.timeline_data.clear();
        for conditional_field in &self.conditional_fields {
            let mut c = conditional_field.0.lock().expect("mutex lock panicked");
            c.clear();
        }
    }

    /// Add a path (e.g., ['data', 'user']) to be used for related user metadata in the timeline.
    ///
    /// # Arguments
    /// - path: A list of strings representing the path to the related user field.
    pub fn add_related_user_ouput_path(&mut self, path: Vec<String>) {
        self.add_field_def(&path, TimelineFieldType::RelatedUser);
    }

    /// Add a path (e.g., ['data', 'desc']) to be used for the primary description in the timeline.
    ///
    /// # Arguments
    /// - path: A list of strings representing the path to the description field.
    pub fn add_description_ouput_path(&mut self, path: Vec<String>) {
        self.add_field_def(&path, TimelineFieldType::DescriptionField);
    }

    /// Add a path (e.g., ['data', 'extra']) to be used for additional description metadata in the timeline.
    ///
    /// # Arguments
    /// - path: A list of strings representing the path to the additional description field.
    pub fn add_additional_description_ouput_path(&mut self, path: Vec<String>) {
        self.add_field_def(&path, TimelineFieldType::AdditionalDescriptionField);
    }

    /// Add a path (e.g., ['data', 'extra']) to be used for additional description metadata in the timeline if the conditional values do not return anything.
    ///
    /// # Arguments
    /// - path: A list of strings representing the path to the default description field.
    pub fn add_otherwise_description_path(&mut self, path: Vec<String>) {
        self.add_field_def(&path, TimelineFieldType::OtherwiseField);
    }

    pub fn add_conditional_description(&mut self, conf: ConditionalDescriptionConf) {
        let mut condition_list = HashMap::with_capacity(conf.conditions.len());
        let mut fields = HashSet::with_capacity(conf.optional_field.len());

        for (path, v) in &conf.conditions {
            if let Some(last) = path.last() {
                condition_list.insert(last.to_string(), v.clone());
            }
        }
        for path in &conf.optional_field {
            if let Some(last) = path.last() {
                fields.insert(last.to_string());
            }
        }

        let conditional = ConditionalDescritionField(Arc::new(Mutex::new(
            ConditionalDescription::new(condition_list, fields),
        )));

        for (path, _) in conf.conditions {
            self.add_conditional_path(path, conditional.clone());
        }
        for path in conf.optional_field {
            self.add_conditional_path(path, conditional.clone());
        }

        self.conditional_fields.push(conditional);
    }
}

impl TimeLineBuilder {
    pub fn add_date(&self, date: DateTime<Utc>, fied_name: &FieldName) {
        let mut data = self.timeline_data.0.lock().expect("mutex lock panicked");

        let meaning = data.timestamps.entry(date).or_insert(TimestampMeaning::new(
            self.timeline_type.clone(),
            self.max_date_meaning,
        ));
        meaning.add_meaning(fied_name);
    }

    pub fn add_field_value(
        &self,
        field_types: &Vec<TimelineFieldType>,
        field_name: &str,
        value: &Value,
    ) {
        let mut data = self.timeline_data.0.lock().expect("mutex lock panicked");
        for field_type in field_types {
            match field_type {
                TimelineFieldType::RelatedUser => {
                    data.related_user_field.push(value.clone());
                }
                TimelineFieldType::DescriptionField => {
                    match data.description_field.get_mut(field_name) {
                        Some(list) => list.push(value.clone()),
                        None => {
                            data.description_field
                                .insert(field_name.to_owned(), vec![value.clone()]);
                        }
                    }
                }
                TimelineFieldType::AdditionalDescriptionField => {
                    match data.additional_description_field.get_mut(field_name) {
                        Some(list) => list.push(value.clone()),
                        None => {
                            data.additional_description_field
                                .insert(field_name.to_owned(), vec![value.clone()]);
                        }
                    }
                }
                TimelineFieldType::ConditionalField { conditional } => {
                    let mut cond = conditional.0.lock().expect("mutex lock panicked");
                    cond.evaluate(field_name, value);
                }

                TimelineFieldType::OtherwiseField => {
                    match data.otherwise_field.get_mut(field_name) {
                        Some(list) => list.push(value.clone()),
                        None => {
                            data.otherwise_field
                                .insert(field_name.to_owned(), vec![value.clone()]);
                        }
                    }
                }
            }
        }
    }

    fn add_conditional_path(&mut self, path: Vec<String>, conditional: ConditionalDescritionField) {
        self.add_field_def(&path, TimelineFieldType::ConditionalField { conditional });
    }

    /// Registers a new field at the given path so the builder knows where to put values later.
    ///
    /// The path is a list of string segments that define a hierarchical structure. This method recursively builds
    /// or updates the field map to include the given field type at the specified path.
    ///
    /// # Arguments
    ///
    /// - `path`: A list of strings representing the hierarchical path to the field.
    /// - `field_type`: The type of field to associate with the path (e.g., `RelatedUser`, `DescriptionField`).
    ///
    /// # Notes
    ///
    /// - If the path is empty, no action is taken.
    /// - If the path has only one element, it is treated as a leaf node and the field type is added to a `Field` list.
    /// - If the path has multiple elements, it recursively builds or updates the nested `Map` structure.
    ///
    fn add_field_def(&mut self, path: &[String], field_type: TimelineFieldType) {
        if path.is_empty() {
            return;
        }
        Self::create_field_definition(path, field_type, &mut self.fields);
    }

    /// Walks the path, creating or updating the nested map as needed.
    ///
    /// This is a helper method used internally by `add_field_def` to build hierarchical field structures.
    ///
    fn create_field_definition(
        path: &[String],
        field_type: TimelineFieldType,
        fields: &mut HashMap<String, TimelineField>,
    ) {
        if path.is_empty() {
            return;
        }
        if path.len() == 1 {
            let name = &path[0];
            let field_opt = fields.remove(name);

            if let Some(mut field) = field_opt {
                field.fields.push(field_type);
                fields.insert(name.clone(), field);
            } else {
                let mut field = TimelineField::new();
                field.fields.push(field_type);
                fields.insert(name.clone(), field);
            }
        } else {
            let name = &path[0];
            let field_opt = fields.remove(name);

            if let Some(mut field) = field_opt {
                Self::create_field_definition(&path[1..], field_type, &mut field.nested);
                fields.insert(name.clone(), field);
            } else {
                let mut field = TimelineField::new();
                Self::create_field_definition(&path[1..], field_type, &mut field.nested);
                fields.insert(name.clone(), field);
            }
        }
    }

    pub fn format_related_user(&self) -> Result<String, Error> {
        let data = self.timeline_data.0.lock().expect("mutex lock panicked");

        let mut descr: String = String::new();
        for value in &data.related_user_field {
            if let Value::Null() = value {
            } else {
                descr = value.to_string(&DateOutputCodec::IsoUtc());
            }
        }
        Ok(descr)
    }

    pub fn format_description(&self) -> Result<String, Error> {
        let mut descr = Vec::new();
        let data = self.timeline_data.0.lock().expect("mutex lock panicked");

        for (name, values) in &data.description_field {
            let mut name_descr = String::new();
            for value in values {
                if let Value::Null() = value {
                } else {
                    if !name_descr.is_empty() {
                        name_descr.push_str(", ");
                    }
                    name_descr.push_str(&value.to_string(&DateOutputCodec::IsoUtc()));
                }
            }

            if !name_descr.is_empty() {
                if self.description_format.include_field_name {
                    descr.push(format!("{name}: {name_descr}"));
                } else {
                    descr.push(name_descr);
                }
            }
        }
        Ok(descr.join(&self.description_format.field_separator))
    }

    pub fn format_additional_description(&mut self) -> Result<String, Error> {
        let mut additional_descr = Vec::new();
        let data = self.timeline_data.0.lock().expect("mutex lock panicked");

        for (name, values) in &data.additional_description_field {
            let mut name_descr = String::new();
            for value in values {
                if let Value::Null() = value {
                } else {
                    if !name_descr.is_empty() {
                        name_descr.push_str(", ");
                    }
                    name_descr.push_str(&value.to_string(&DateOutputCodec::IsoUtc()));
                }
            }

            if !name_descr.is_empty() {
                if self.additional_description_format.include_field_name {
                    additional_descr.push(format!("{name}: {name_descr}"));
                } else {
                    additional_descr.push(name_descr);
                }
            }
        }
        for cond in &self.conditional_fields {
            let mut cond = cond.0.lock().expect("mutex lock panicked");
            if let Some(values) = cond.drain_values() {
                for (name, value) in values {
                    if let Value::Null() = value {
                    } else {
                        let string_val = value.to_string(&DateOutputCodec::IsoUtc());
                        if self.additional_description_format.include_field_name {
                            additional_descr.push(format!("{name}: {string_val}"));
                        } else {
                            additional_descr.push(string_val);
                        }
                    }
                }
            }
        }
        if additional_descr.is_empty() {
            for (name, values) in &data.otherwise_field {
                let mut name_descr = String::new();
                for value in values {
                    if let Value::Null() = value {
                    } else {
                        if !name_descr.is_empty() {
                            name_descr.push_str(", ");
                        }
                        name_descr.push_str(&value.to_string(&DateOutputCodec::IsoUtc()));
                    }
                }

                if !name_descr.is_empty() {
                    if self.additional_description_format.include_field_name {
                        additional_descr.push(format!("{name}: {name_descr}"));
                    } else {
                        additional_descr.push(name_descr);
                    }
                }
            }
        }

        Ok(additional_descr.join(&self.additional_description_format.field_separator))
    }

    pub fn to_timelines(&mut self, timeline_records: &mut Vec<TimeLine>) -> Result<(), Error> {
        let related_user = self.format_related_user()?;
        let description = self.format_description()?;
        let additional_description = self.format_additional_description()?;
        let mut timeline_data = self.timeline_data.0.lock().expect("mutex lock panicked");
        for (timestamp, meaning) in timeline_data.timestamps.drain() {
            let timestamp_meaning = meaning.format_meaning();
            let data_type = self.data_type.clone();
            let related_user = related_user.clone();
            let description = description.clone();
            let additional_description = additional_description.clone();

            timeline_records.push(TimeLine {
                timestamp,
                timestamp_meaning,
                data_type,
                related_user,
                description,
                additional_description,
                ..Default::default()
            });
        }
        drop(timeline_data);
        self.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use crate::{
        DateInputCodec, FieldMapping, Metadata, Parser, Qualifiers, Record,
        date_util::DateOutputCodec, field::Field, line_builder::LineBuilder,
    };

    use super::*;

    /// Verify that two timestamps from separate fields are merged correctly, and that user, description and additional description fields appear as expected. Also checks the MACB meaning string.
    #[test]
    fn timeline() {
        let mut timeline: TimeLineBuilder =
            TimeLineBuilder::new(TimeLineType::MacbMacb, "evtx".to_owned(), 0, None, None);

        let date =
            crate::date_util::parse_date("1996-12-19T16:39:57.123-08:00", &DateInputCodec::Iso())
                .unwrap();

        let datefield = FieldName::new(
            "date_1".to_owned(),
            false,
            None,
            None,
            Some("Meaning 1".to_owned()),
            Some("Long Meaning 1".to_owned()),
        );
        timeline.add_date(date, &datefield);

        let datefield_2 = FieldName::new(
            "date_2".to_owned(),
            false,
            None,
            None,
            Some("Meaning 2".to_owned()),
            Some("Long Meaning 2".to_owned()),
        );
        timeline.add_date(date, &datefield_2);

        timeline.add_field_value(
            &vec![TimelineFieldType::DescriptionField],
            "int",
            &Value::Int(10),
        );
        timeline.add_field_value(
            &vec![TimelineFieldType::DescriptionField],
            "data",
            &Value::String("some string".to_owned()),
        );

        timeline.add_field_value(
            &vec![TimelineFieldType::RelatedUser],
            "data",
            &Value::String("{012-564-23}".to_owned()),
        );

        timeline.add_field_value(
            &vec![TimelineFieldType::AdditionalDescriptionField],
            "additional",
            &Value::String("some data".to_owned()),
        );

        assert_eq!("{012-564-23}", timeline.format_related_user().unwrap());
        assert_eq!(
            "int: 10 - data: some string",
            timeline.format_description().unwrap()
        );
        assert_eq!(
            "additional: some data",
            timeline.format_additional_description().unwrap()
        );
        let timeline_data = timeline
            .timeline_data
            .0
            .lock()
            .expect("mutex lock panicked");
        for (_, meaning) in &timeline_data.timestamps {
            assert_eq!("Meaning 1 - Meaning 2", meaning.format_meaning())
        }
    }

    #[test]
    fn builder_definition() {
        let mut timeline: TimeLineBuilder =
            TimeLineBuilder::new(TimeLineType::MacbMacb, "evtx".to_owned(), 0, None, None);

        // Add field definitions using both name and path methods for different types
        timeline.add_related_user_ouput_path(vec!["user".to_owned()]);
        timeline.add_description_ouput_path(vec!["descr".to_owned()]);
        timeline.add_additional_description_ouput_path(vec!["additional".to_owned()]);

        // Add nested paths for the same fields to test hierarchical structure
        timeline.add_description_ouput_path(vec!["data".to_owned(), "descr".to_owned()]);
        timeline.add_related_user_ouput_path(vec!["data".to_owned(), "descr".to_owned()]);

        // Verify that the root-level fields are correctly added
        assert!(timeline.fields.contains_key("additional"));

        // Validate nested structure
        let field = timeline.fields.get("data").unwrap();
        assert!(field.nested.contains_key("descr"));

        // Test adding a date to the timeline and verify it works
        let date =
            crate::date_util::parse_date("1996-12-19T16:39:57.123-08:00", &DateInputCodec::Iso())
                .unwrap();

        let datefield = FieldName::new(
            "input".to_owned(),
            false,
            None,
            None,
            None,
            Some("Meaning of date 1".to_owned()),
        );
        timeline.add_date(date, &datefield);
    }

    /// Tests the MACBMACB timestamp formatting.
    ///
    /// This test creates a timeline with the `MacbMacb` type and adds four timestamps from different fields.
    /// It then verifies that the MACB encoding correctly maps these fields
    #[test]
    fn macbmacb() {
        let timeline: TimeLineBuilder =
            TimeLineBuilder::new(TimeLineType::MacbMacb, "evtx".to_owned(), 0, None, None);

        let date =
            crate::date_util::parse_date("1996-12-19T16:39:57.123-08:00", &DateInputCodec::Iso())
                .unwrap();

        timeline.add_date(
            date,
            &FieldName::new("si_lastmod_date".to_owned(), false, None, None, None, None),
        );
        timeline.add_date(
            date,
            &FieldName::new("si_creation_date".to_owned(), false, None, None, None, None),
        );
        timeline.add_date(
            date,
            &FieldName::new(
                "fn_lastaccess_date".to_owned(),
                false,
                None,
                None,
                None,
                None,
            ),
        );
        timeline.add_date(
            date,
            &FieldName::new(
                "fn_lastchange_date".to_owned(),
                false,
                None,
                None,
                None,
                None,
            ),
        );
        let timeline_data = timeline
            .timeline_data
            .0
            .lock()
            .expect("mutex lock panicked");
        for (_, meaning) in &timeline_data.timestamps {
            assert_eq!("$SI:M..B - $FN:.AC.", meaning.format_meaning())
        }
    }

    #[test]
    fn nested_timeline() {
        //
        // field mapping
        //
        let qualifiers = Qualifiers::new();
        let lvl2_name = FieldName::new(
            "lvl2".to_owned(),
            false,
            Some("lvl2_output".to_owned()),
            Some(qualifiers.COMPANY),
            None,
            None,
        );
        let level2_greetings = Field::Single {
            name: FieldName::new(
                "greetings".to_owned(),
                false,
                Some("lvl2_greeting".to_owned()),
                Some(qualifiers.APP_NAME),
                None,
                Some("greetings desc".to_owned()),
            ),
            parser: Parser::String(),
            default_value: None,
        };
        let level2_field = Field::Single {
            name: FieldName::new(
                "descr".to_owned(),
                false,
                Some("lvl2_descr".to_owned()),
                None,
                None,
                Some("lvl2_descr".to_owned()),
            ),
            parser: Parser::String(),
            default_value: None,
        };

        let level_2 = Field::Object {
            name: lvl2_name,
            ignore: false,
            fields: vec![
                level2_greetings.clone(),
                level2_field.clone(),
                Field::Single {
                    name: FieldName::new(
                        "lastmod_date".to_owned(),
                        false,
                        Some("si_lastmod_date".to_owned()),
                        None,
                        None,
                        Some("Windows UserId".to_owned()),
                    ),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
        };

        let level_1_name = FieldName::new(
            "lvl1".to_owned(),
            false,
            Some("lvl1_output".to_owned()),
            Some(qualifiers.FILE_NAME),
            None,
            None,
        );

        let level1_field = Field::Single {
            name: FieldName::new(
                "descr".to_owned(),
                false,
                Some("lvl1_descr".to_owned()),
                None,
                None,
                Some("lvl1_descr".to_owned()),
            ),
            parser: Parser::String(),
            default_value: None,
        };

        let level_1 = Field::Object {
            name: level_1_name,
            ignore: false,
            fields: vec![
                level_2.clone(),
                level1_field,
                Field::Single {
                    name: FieldName::new(
                        "lastmod_date".to_owned(),
                        false,
                        Some("fn_lastchange_date".to_owned()),
                        None,
                        None,
                        Some("Windows UserId".to_owned()),
                    ),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
        };

        let user_id = Field::Single {
            name: FieldName::new(
                "user".to_owned(),
                false,
                Some("user_id".to_owned()),
                Some(qualifiers.CERT_SHA1),
                None,
                Some("Windows UserId".to_owned()),
            ),
            parser: Parser::String(),
            default_value: None,
        };

        let mapping = vec![
            user_id.clone(),
            level_1.clone(),
            Field::Single {
                name: FieldName::new(
                    "creation_date_si".to_owned(),
                    false,
                    Some("si_creation_date".to_owned()),
                    Some(qualifiers.DATE_CREATION),
                    None,
                    Some("Creation date".to_owned()),
                ),
                parser: Parser::String(),
                default_value: None,
            },
            Field::Single {
                name: FieldName::new(
                    "fn_creationDate".to_owned(),
                    false,
                    Some("fn_creation_date".to_owned()),
                    None,
                    None,
                    Some("Creation date".to_owned()),
                ),
                parser: Parser::String(),
                default_value: None,
            },
        ];

        let field_mapping = FieldMapping::new(mapping, None);

        //
        // Timeline Definition
        //
        let mut timeline: TimeLineBuilder =
            TimeLineBuilder::new(TimeLineType::MacbMacb, "ntfs".to_owned(), 0, None, None);

        timeline.add_related_user_ouput_path(vec![user_id.output_name().to_owned()]);
        timeline
            .add_description_ouput_path(vec!["lvl1_output".to_owned(), "lvl1_descr".to_owned()]);

        timeline.add_description_ouput_path(vec![
            "lvl1_output".to_owned(),
            "lvl2_output".to_owned(),
            "lvl2_descr".to_owned(),
        ]);

        let metadata = Metadata::new("test".into());
        let mut line_builder = LineBuilder::new(
            metadata,
            Some(timeline),
            field_mapping,
            true,
            false,
            false,
            true,
        );

        let mut data = Record::new();

        let date =
            crate::date_util::parse_date("1990-01-10T14:19:21.123-08:00", &DateInputCodec::Iso())
                .unwrap();
        data.add("si_creation_date", Value::Date(date.clone()));
        data.add("fn_creation_date", Value::Date(date.clone()));

        let date =
            crate::date_util::parse_date("1996-12-19T16:39:57.123-08:00", &DateInputCodec::Iso())
                .unwrap();

        data.add(
            user_id.output_name(),
            Value::String("{012-564-23}".to_owned()),
        );

        let mut level_2_data = Record::new();
        level_2_data.add(
            level2_greetings.output_name(),
            Value::String("Hello Lvl2".to_owned()),
        );

        level_2_data.add("l2_not_mapped", Value::Bool(true));
        level_2_data.add("si_lastmod_date", Value::Date(date.clone()));
        level_2_data.add(
            level2_field.output_name(),
            Value::String("level 2 descrp".to_owned()),
        );

        let mut level_1_data = Record::new();

        level_1_data.add(level_2.output_name(), Value::Object(level_2_data));

        level_1_data.add("l1_not_mapped", Value::Bool(true));
        level_1_data.add("fn_lastchange_date", Value::Date(date.clone()));
        level_1_data.add("lvl1_descr", Value::String("level 1 descrp".to_owned()));

        data.add(level_1.output_name(), Value::Object(level_1_data));
        data.add("not_mapped", Value::Bool(true));

        line_builder.build(&mut data).unwrap();

        assert_eq!(line_builder.line_data.timeline.len(), 2);

        let timeline = &line_builder.line_data.timeline[0];

        if !(timeline.timestamp_meaning.eq("$SI:...B - $FN:...B")
            || timeline.timestamp_meaning.eq("$SI:M... - $FN:..C."))
        {
            panic!("invalid macb: {}", timeline.timestamp_meaning)
        }

        assert_eq!("{012-564-23}", timeline.related_user);

        assert_eq!(
            "lvl2_descr: level 2 descrp - lvl1_descr: level 1 descrp",
            timeline.description
        );
    }
    /// Test the basic `TimeLine` struct serialization and ID computation.
    #[test]
    fn timeline_struct_serialisation() {
        let mut tl = TimeLine::default();
        tl.computer = Some("COMP1".to_owned());
        tl.timestamp =
            crate::date_util::parse_date("2020-01-01T00:00:00Z", &DateInputCodec::Iso()).unwrap();
        tl.timestamp_meaning = "Created".to_owned();
        tl.data_type = "evtx".to_owned();
        tl.related_user = "user".to_owned();
        tl.description = "desc".to_owned();
        tl.additional_description = "add".to_owned();

        // Compute deterministic ID.
        let mut hasher = blake3::Hasher::new();
        tl.compute_id(&mut hasher);
        assert!(tl.id.is_some(), "ID should be set after compute_id");

        // JSON serialisation.
        let mut json = String::new();
        tl.json_serialise(&mut json, &DateOutputCodec::IsoUtc(), true)
            .unwrap();
        assert!(
            json.contains("\"computer\":\"COMP1\""),
            "JSON should contain computer"
        );
        assert!(
            json.contains("\"timestamp_meaning\":\"Created\""),
            "JSON should contain timestamp meaning"
        );
        assert!(
            json.contains("\"id\":\"HDsvocVTyj222XnsHSuRYturmDt5wJ2NXtS2R9zNsKM=\""),
            "JSON should contain timestamp meaning"
        );

        let normalized = true;
        // CSV serialisation (normalized).
        let mut csv = String::new();
        TimeLine::csv_serialise_header(&mut csv, normalized);
        csv.push_str("\n");
        tl.csv_serialise(&mut csv, &DateOutputCodec::IsoUtc(), true, normalized)
            .unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(csv.as_bytes()));
        let mut line_number = 0;
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();
            assert_eq!(10, rec.len());
            assert_eq!(&rec[0], "HDsvocVTyj222XnsHSuRYturmDt5wJ2NXtS2R9zNsKM=");
            assert_eq!(&rec[1], "COMP1");
            assert_eq!(&rec[2], "2020-01-01T00:00:00.000000+00:00");
            assert_eq!(&rec[3], "Created");
            assert_eq!(&rec[4], "evtx");
            assert_eq!(&rec[5], "user");
            assert_eq!(&rec[6], "desc");
            assert_eq!(&rec[7], "add");
            assert_eq!(&rec[8], "");
            assert_eq!(&rec[9], "");
        }
        assert_eq!(1, line_number);

        let normalized = false;
        let mut csv = String::new();
        TimeLine::csv_serialise_header(&mut csv, normalized);
        csv.push_str("\n");
        tl.csv_serialise(&mut csv, &DateOutputCodec::IsoUtc(), true, normalized)
            .unwrap();

        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .delimiter(CSV_DELIMITER as u8)
            .from_reader(Box::new(csv.as_bytes()));
        let mut line_number = 0;
        for (line_nb, record) in reader.records().enumerate() {
            line_number = line_nb + 1;
            let rec = record.unwrap();
            assert_eq!(9, rec.len());
            assert_eq!(&rec[0], "COMP1");
            assert_eq!(&rec[1], "2020-01-01T00:00:00.000000+00:00");
            assert_eq!(&rec[2], "Created");
            assert_eq!(&rec[3], "evtx");
            assert_eq!(&rec[4], "user");
            assert_eq!(&rec[5], "desc");
            assert_eq!(&rec[6], "add");
            assert_eq!(&rec[7], "");
            assert_eq!(&rec[8], "");
        }
        assert_eq!(1, line_number);
    }
}
