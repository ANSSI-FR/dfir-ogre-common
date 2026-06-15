use std::collections::HashMap;

use blake3::Hasher;

use crate::{
    Field, FieldMapping, FieldName, Metadata, Record, Value,
    errors::Error,
    timeline::{TimeLine, TimeLineBuilder, TimelineField},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE as BASE64};
/// Internal field that holds optional metadata attached to each record.
pub const OUTPUT_METADATA_FIELD: &str = "ogre_md";
pub const OUTPUT_METADATA_ID_FIELD: &str = "ogre_md_id";
pub const OUPUT_DATA_ID: &str = "ogre_id";

/// Holds the processed data for a single log line, including main data and timeline entries.
///
/// `LineData` is used internally by [`LineBuilder`] to accumulate transformed fields
/// from parsed input records. It can optionally compute a hash identifier based on
/// primary keys or all fields if no primary key is defined.
#[derive(Clone)]
pub struct LineData {
    /// The main data record containing transformed field-value pairs.
    pub data: Record,

    pub compute_hash: bool,
    pub data_id: Option<String>,

    /// hasher used to generate a unique identifier from primary keys or all fields.
    pub data_id_hasher: Hasher,
    /// Vector of timeline records generated from date fields during parsing.
    pub timeline: Vec<TimeLine>,
    /// If `true`, field names will include their qualifier prefixes (e.g., "source.field").
    pub require_qualifiers: bool,

    /// Indicates whether the input data contains defined primary keys for hashing purposes.
    pub has_primary_keys: bool,
}
impl LineData {
    /// Creates a new `LineData` instance with default empty collections.
    ///
    /// # Arguments
    ///
    /// * `compute_hash` - If true, initializes a hasher to generate identifiers from field values.
    /// * `require_qualifiers` - If true, field names will include their qualifier suffixes.
    /// * `has_primary_keys` - Indicates if the input has defined primary keys for hashing.
    fn new(compute_hash: bool, require_qualifiers: bool, has_primary_keys: bool) -> Self {
        Self {
            has_primary_keys,
            data: Record::new(),
            data_id: None,
            timeline: Vec::new(),
            data_id_hasher: Hasher::new(),
            compute_hash,
            require_qualifiers,
        }
    }

    /// Creates a new `LineData` with pre-allocated capacity for the data record.
    ///
    /// # Arguments
    ///
    /// * `size` - Initial capacity for the internal data record.
    /// * `require_qualifiers` - If true, field names will include their qualifier prefixes.
    /// * `compute_hash` - If true, initializes a hasher to generate identifiers from field values.
    /// * `has_primary_keys` - Indicates if the input has defined primary keys for hashing.
    fn with_capacity(
        size: usize,
        require_qualifiers: bool,
        compute_hash: bool,
        has_primary_keys: bool,
    ) -> Self {
        Self {
            require_qualifiers,
            has_primary_keys,
            data: Record::with_capacity(size),
            data_id: None,
            timeline: Vec::new(),
            data_id_hasher: Hasher::new(),
            compute_hash,
        }
    }

    /// Adds a field-value pair to the data record and updates the hasher if compute_hash is enabled.
    pub fn add_data(&mut self, field_name: &FieldName, value: Value) {
        if self.compute_hash {
            if self.has_primary_keys {
                if field_name.primary_key {
                    self.data_id_hasher
                        .update(field_name.output_name().as_bytes());
                    value.hash(&mut self.data_id_hasher);
                }
            } else {
                //if no primary key is defined, every fields are used to create the identifier
                self.data_id_hasher
                    .update(field_name.output_name().as_bytes());
                value.hash(&mut self.data_id_hasher);
            }
        }

        self.data
            .add(field_name.name(self.require_qualifiers), value);
    }

    /// Clears all data and timeline records, resetting the hasher if present.
    /// Clears all data and timeline records, resetting the hasher if present.
    fn clear(&mut self) {
        self.data_id = None;
        self.data.clear();
        self.timeline.clear();
    }
}

/// Builds a structured log line from input data using configurable field mappings.
///
/// `LineBuilder` takes raw input records and transforms them into a standardized format
/// based on the provided [`FieldMapping`], handling flat fields, nested objects, and arrays.
/// It also supports timeline generation for date fields and snake_case key conversion.
#[derive(Clone)]
pub struct LineBuilder {
    /// Metadata associated with this log line (e.g., source info, parsing configuration).
    pub metadata: Metadata,
    /// Unique identifier generated from the hash of the metadata record.
    pub metadata_id: String,
    /// Optional builder responsible for generating timeline entries from date fields.
    pub timeline_builder: Option<TimeLineBuilder>,
    /// Configuration defining how input fields map to output field names and structure.
    pub field_mapping: FieldMapping,
    /// If `true`, unmapped field keys will be converted to snake_case (e.g., "CamelCase" → "camel_case").
    pub force_snake_case: bool,
    /// Internal state holding the current line's processed data and timeline records.
    pub line_data: LineData,
    /// If `true`, compute deterministic identifiers from metadata and line data.
    pub compute_hash: bool,
}

impl LineBuilder {
    /// Creates a new `LineBuilder` instance with the specified configuration.
    ///
    /// # Arguments
    ///
    /// * `metadata` - The metadata record to associate with each generated line.
    /// * `timeline_builder` - Optional timeline builder for date field enrichment.
    /// * `field_mapping` - Configuration defining how input fields map to output format.
    /// * `require_qualifiers` - If true, field names will include their qualifier prefixes.
    /// * `compute_hash` - If true, enables hash-based identifier generation from field values.
    /// * `has_primary_keys` - Indicates if the input data contains defined primary keys.
    /// * `force_snake_case` - If true, converts unmapped field keys to snake_case.
    ///
    /// # Returns
    ///
    /// A new `LineBuilder` instance configured with the specified parameters.
    pub fn new(
        mut metadata: Metadata,
        timeline_builder: Option<TimeLineBuilder>,
        field_mapping: FieldMapping,
        require_qualifiers: bool,
        compute_hash: bool,
        has_primary_keys: bool,
        force_snake_case: bool,
    ) -> Self {
        let mut metadata_hasher = Hasher::new();
        metadata.compute_id(&mut metadata_hasher);
        let metadata_hash = metadata_hasher.finalize();
        let metadata_id = BASE64.encode(metadata_hash.as_slice());
        if compute_hash {
            metadata.id = Some(metadata_id.clone())
        }

        Self {
            metadata,
            metadata_id,
            timeline_builder,
            field_mapping,
            force_snake_case,
            line_data: LineData::new(compute_hash, require_qualifiers, has_primary_keys),
            compute_hash,
        }
    }

    /// Resets the internal line data for re-use of this builder without creating a new instance.
    pub fn clear_data(&mut self) {
        self.line_data.clear();
    }

    /// Processes input data according to the configured field mappings and produces structured output.
    ///
    /// This method clears previous data, applies all field mappings (including nested objects
    /// and arrays), and optionally populates the timeline based on date fields.
    ///
    /// # Arguments
    ///
    /// * `input_data` - The mutable input record to transform.
    ///
    /// # Errors
    ///
    /// Returns an error if array processing encounters unsupported nesting levels or other issues.
    pub fn build(&mut self, input_data: &mut Record) -> Result<(), Error> {
        self.clear_data();

        let parsing_tree = &self.field_mapping.field_parser_tree;
        let timeline_builder = self.timeline_builder.as_ref();
        let timeline_fields = timeline_builder.map(|tl| &tl.fields);
        Self::build_record(
            input_data,
            &mut self.line_data,
            timeline_builder,
            timeline_fields,
            &parsing_tree.output_fields,
            self.force_snake_case,
            true,
        )?;
        if let Some(builder) = &mut self.timeline_builder {
            builder.to_timelines(&mut self.line_data.timeline)?;
        }

        if self.compute_hash {
            //add some metadata to the data id to ensure uniqueness across computer, original file name and vss
            self.line_data
                .data_id_hasher
                .update(self.metadata.computer.as_bytes());
            if let Some(filename) = &self.metadata.original_filename {
                self.line_data.data_id_hasher.update(filename.as_bytes());
            }
            if let Some(vss) = &self.metadata.vss {
                self.line_data.data_id_hasher.update(vss.as_bytes());
            }

            let data_id = BASE64.encode(self.line_data.data_id_hasher.finalize().as_slice());

            self.line_data.data_id = Some(data_id.clone());
            self.line_data
                .data
                .add(OUPUT_DATA_ID, Value::String(data_id.clone()));

            let mut hasher = Hasher::new();
            for timeline in &mut self.line_data.timeline {
                timeline.data_id = Some(data_id.clone());
                timeline.metadata_id = Some(self.metadata_id.clone());
                timeline.computer = Some(self.metadata.computer.clone());
                timeline.compute_id(&mut hasher);
                hasher.reset();
            }
        }

        Ok(())
    }

    /// Recursively processes input data and populates `line_data` according to the mapping rules.
    ///
    /// This is the core transformation logic that handles:
    /// - Flat field mappings (direct field-to-field transformation)
    /// - Array mappings (flat arrays, object arrays, nested arrays)
    /// - Nested object mappings (embedding sub-records)
    /// - Unmapped fields (fields not explicitly mapped are included with optional snake_case conversion)
    ///
    /// # Arguments
    ///
    /// * `input_data` - The mutable input record to transform.
    /// * `line_data` - The output structure where transformed data is accumulated.
    /// * `timeline_builder` - Optional timeline builder to enrich date fields.
    /// * `timeline_fields` - Mapping of field names to their timeline configurations.
    /// * `parsing_tree` - The field parser tree defining the transformation rules.
    /// * `force_snake_case` - Whether to convert unmapped keys to snake_case.
    /// * `root` - Indicates if this is the root level (controls inclusion of all fields).
    ///
    pub fn build_record(
        input_data: &mut Record,
        line_data: &mut LineData,
        timeline_builder: Option<&TimeLineBuilder>,
        timeline_fields: Option<&HashMap<String, TimelineField>>,
        field_mapping: &Vec<Field>,
        force_snake_case: bool,
        root: bool,
    ) -> Result<(), Error> {
        // Iterate over each field mapping definition
        for field in field_mapping {
            match field {
                Field::Single {
                    name,
                    parser: _,
                    default_value: _,
                } => {
                    Self::process_flat_field(
                        input_data,
                        line_data,
                        timeline_builder,
                        timeline_fields,
                        name,
                        field.ignore(),
                        root,
                    );
                }
                Field::Multi(multi_input_field) => {
                    Self::process_flat_field(
                        input_data,
                        line_data,
                        timeline_builder,
                        timeline_fields,
                        &multi_input_field.output_field,
                        field.ignore(),
                        root,
                    );
                }
                Field::Array(array_field) => {
                    let inner_field = array_field.0.as_ref();
                    Self::process_array_field_mapping(
                        input_data,
                        line_data,
                        inner_field,
                        timeline_builder,
                        timeline_fields,
                        force_snake_case,
                    )?;
                }
                Field::Object {
                    name,
                    fields,
                    ignore,
                } => {
                    Self::process_object_field_mapping(
                        input_data,
                        line_data,
                        timeline_builder,
                        timeline_fields,
                        name,
                        fields,
                        *ignore,
                        force_snake_case,
                    )?;
                }
            }
        }

        // Process any remaining unmapped fields from the input data
        for (key, value) in input_data.drain() {
            Self::process_unmapped_field(
                key,
                value,
                line_data,
                timeline_builder,
                timeline_fields,
                force_snake_case,
            )?;
        }

        Ok(())
    }

    /// Processes a single flat field mapping from the parser tree.
    ///
    /// Removes the field value from `input_data`, adds it to `line_data`,
    /// and optionally enriches the timeline with date information.
    ///
    /// # Arguments
    ///
    /// * `input_data` - The mutable input record to read values from.
    /// * `line_data` - The output structure where transformed data is accumulated.
    /// * `timeline_builder` - Optional timeline builder for date enrichment.
    /// * `timeline_fields` - Mapping of field names to timeline configurations.
    /// * `field_name` - The target field name with qualifiers and output name info.
    /// * `ignore` - If true, the field is skipped (but still removed from input).
    /// * `root` - Indicates if this is the root level (controls default inclusion).
    fn process_flat_field(
        input_data: &mut Record,
        line_data: &mut LineData,
        timeline_builder: Option<&TimeLineBuilder>,
        timeline_fields: Option<&HashMap<String, TimelineField>>,
        field_name: &FieldName,
        ignore: bool,
        root: bool,
    ) {
        let output_key = field_name.output_name();

        let data = input_data.remove(output_key);
        if ignore {
            //early return if the field must be ignored (but we make sure that the data is properly remove from the input record)
            return;
        }
        if let Some(value) = data {
            // If the value is a date, add it to the timeline
            if let Value::Date(date) = &value
                && let Some(timeline_builder) = timeline_builder
            {
                timeline_builder.add_date(*date, field_name);
            } else if let Value::Null() = &value {
                line_data.add_data(field_name, value);
                return; //return early to avoid inserting empty fields in the timeline data
            }

            // Enrich the timeline with the field if necessary
            if let Some(timeline_builder) = timeline_builder
                && let Some(tl_fields) = timeline_fields
                && let Some(tl_field) = tl_fields.get(output_key)
            {
                timeline_builder.add_field_value(&tl_field.fields, field_name.name(false), &value);
            }

            line_data.add_data(field_name, value);
        } else if root {
            //for the root elements, always include every fields
            line_data.add_data(field_name, Value::Null());
        }
    }

    /// Processes object-level field mappings, recursively processing nested data.
    ///
    /// # Arguments
    ///
    /// * `input_data` - The mutable input record to read values from.
    /// * `line_data` - The output structure where transformed data is accumulated.
    /// * `timeline_builder` - Optional timeline builder for date enrichment.
    /// * `timeline_fields` - Mapping of field names to timeline configurations.
    /// * `field_parsers` - Parser tree defining how nested fields should be mapped.
    /// * `force_snake_case` - Whether to convert unmapped keys to snake_case.
    ///
    fn process_object_field_mapping(
        input_data: &mut Record,
        line_data: &mut LineData,
        timeline_builder: Option<&TimeLineBuilder>,
        timeline_fields: Option<&HashMap<String, TimelineField>>,
        field_name: &FieldName,
        field_mapping: &Vec<Field>,
        ignore_parsing: bool,
        force_snake_case: bool,
    ) -> Result<(), Error> {
        let value_opt = input_data.remove(field_name.output_name());

        if ignore_parsing {
            return Ok(());
        }

        if let Some(value) = value_opt {
            let inner_tl_fields = timeline_fields.and_then(|tl| tl.get(field_name.output_name()));

            let inner_tl_fields = if let Some(tlfields) = inner_tl_fields {
                if let Some(timeline_builder) = timeline_builder
                    && !tlfields.fields.is_empty()
                {
                    timeline_builder.add_field_value(
                        &tlfields.fields,
                        field_name.name(false),
                        &value,
                    );
                }
                Some(&tlfields.nested)
            } else {
                None
            };

            if let Value::Object(mut inner_data) = value {
                let mut inner_insert = LineData::with_capacity(
                    field_mapping.len(),
                    line_data.require_qualifiers,
                    false,
                    false,
                );
                // Recursive processing
                Self::build_record(
                    &mut inner_data,
                    &mut inner_insert,
                    timeline_builder,
                    inner_tl_fields,
                    field_mapping,
                    force_snake_case,
                    false,
                )?;

                line_data.add_data(field_name, Value::Object(inner_insert.data));
            }
        }

        Ok(())
    }

    /// Processes array field mappings, including flat arrays and object arrays.
    ///
    /// This handles:
    /// - Flat arrays of primitive values
    /// - Nested object arrays (converts each element using the same mapping rules)
    /// - Rejects unsupported deep nesting levels for arrays
    ///
    /// # Arguments
    ///
    /// * `input_data` - The mutable input record to read values from.
    /// * `line_data` - The output structure where transformed data is accumulated.
    /// * `input_name` - Original name of the array field in the input.
    /// * `array_parser_type` - The parser configuration for this array type.
    /// * `timeline_builder` - Optional timeline builder for date enrichment.
    /// * `timeline_fields` - Mapping of field names to timeline configurations.
    /// * `force_snake_case` - Whether to convert unmapped keys to snake_case.
    fn process_array_field_mapping(
        input_data: &mut Record,
        line_data: &mut LineData,
        inner_field: &Field,
        timeline_builder: Option<&TimeLineBuilder>,
        timeline_fields: Option<&HashMap<String, TimelineField>>,
        force_snake_case: bool,
    ) -> Result<(), Error> {
        match inner_field {
            Field::Single {
                name,
                parser: _,
                default_value: _,
            } => {
                let output_name = name.output_name();
                let value_opt = input_data.remove(output_name);

                let result_array = if let Some(Value::Array(value_array)) = value_opt {
                    value_array
                } else {
                    vec![]
                };

                let value_array = Value::Array(result_array);

                // timeline enrichment (if required)
                if let Some(timeline_builder) = timeline_builder
                    && let Some(tl_fields) = timeline_fields
                    && let Some(fields) = tl_fields.get(output_name)
                {
                    timeline_builder.add_field_value(
                        &fields.fields,
                        name.name(false),
                        &value_array,
                    );
                }

                line_data.add_data(name, value_array);
                Ok(())
            }
            Field::Multi(multi_input_field) => {
                let output_name = multi_input_field.output_field.output_name();
                let value_opt = input_data.remove(output_name);

                let result_array = if let Some(Value::Array(value_array)) = value_opt {
                    value_array
                } else {
                    vec![]
                };

                let value_array = Value::Array(result_array);

                // timeline enrichment (if required)
                if let Some(timeline_builder) = timeline_builder
                    && let Some(tl_fields) = timeline_fields
                    && let Some(fields) = tl_fields.get(output_name)
                {
                    timeline_builder.add_field_value(
                        &fields.fields,
                        multi_input_field.output_field.name(false),
                        &value_array,
                    );
                }

                line_data.add_data(&multi_input_field.output_field, value_array);
                Ok(())
            }
            Field::Array(array_field) => {
                Self::process_array_field_mapping(
                    input_data,
                    line_data,
                    &array_field.0,
                    timeline_builder,
                    timeline_fields,
                    force_snake_case,
                )?;
                Ok(())
            }
            Field::Object {
                name,
                fields,
                ignore,
            } => {
                let output_name = name.output_name();
                let value_opt = input_data.remove(output_name);

                if *ignore {
                    return Ok(());
                }

                let mut result_array = vec![];

                if let Some(Value::Array(value_array)) = value_opt {
                    if let Some(timeline_builder) = timeline_builder
                        && let Some(tl_fields) = timeline_fields
                        && let Some(fields) = tl_fields.get(output_name)
                    {
                        let array = Value::Array(value_array.clone());
                        timeline_builder.add_field_value(&fields.fields, name.name(false), &array);
                    }

                    for v in value_array {
                        if let Value::Object(mut inner_data) = v {
                            let inner_tl_fields = timeline_fields
                                .and_then(|tl| tl.get(output_name))
                                .map(|tf| &tf.nested);

                            let mut inner_insert = LineData::with_capacity(
                                fields.len(),
                                line_data.require_qualifiers,
                                false,
                                false,
                            );
                            // Recursive processing
                            Self::build_record(
                                &mut inner_data,
                                &mut inner_insert,
                                timeline_builder,
                                inner_tl_fields,
                                fields,
                                force_snake_case,
                                false,
                            )?;
                            result_array.push(Value::Object(inner_insert.data));
                        }
                    }
                }

                line_data.add_data(name, Value::Array(result_array));

                Ok(())
            }
        }
    }

    /// Processes fields that were not matched by any explicit mapping rule.
    ///
    /// Unmapped fields are added to the output with optional snake_case conversion.
    /// If a timeline builder is present, date values are added to the timeline.
    ///
    /// # Arguments
    ///
    /// * `key` - The original key name from the input data.
    /// * `value` - The value associated with the key.
    /// * `line_data` - The output structure where transformed data is accumulated.
    /// * `timeline_builder` - Optional timeline builder for date enrichment.
    /// * `timeline_fields` - Mapping of field names to timeline configurations.
    /// * `force_snake_case` - Whether to convert unmapped keys to snake_case.
    fn process_unmapped_field(
        key: String,
        value: Value,
        line_data: &mut LineData,
        timeline_builder: Option<&TimeLineBuilder>,
        timeline_fields: Option<&HashMap<String, TimelineField>>,
        force_snake_case: bool,
    ) -> Result<(), Error> {
        let insert_key = match force_snake_case {
            true => to_snake_case(&key),
            false => key,
        };
        match value {
            Value::Object(mut inner_data) => {
                let mut inner_insert = LineData::with_capacity(
                    inner_data.len(),
                    line_data.require_qualifiers,
                    false,
                    false,
                );
                // Recursive processing
                Self::build_record(
                    &mut inner_data,
                    &mut inner_insert,
                    timeline_builder,
                    None,
                    &vec![],
                    force_snake_case,
                    true,
                )?;

                line_data.add_data(
                    &FieldName::new(insert_key, false, None, None, None, None),
                    Value::Object(inner_insert.data),
                );
            }
            _ => {
                if let Some(timeline_builder) = timeline_builder {
                    // If the value is a date, add it to the timeline
                    if let Value::Date(date) = &value {
                        timeline_builder.add_date(
                            *date,
                            &FieldName::new(insert_key.to_owned(), false, None, None, None, None),
                        );
                    } else if let Value::Null() = &value {
                        return Ok(());
                    }

                    if let Some(tl_fields) = timeline_fields
                        && let Some(tl_field) = tl_fields.get(&insert_key)
                    {
                        timeline_builder.add_field_value(&tl_field.fields, &insert_key, &value);
                    }
                }

                line_data.add_data(
                    &FieldName::new(insert_key, false, None, None, None, None),
                    value,
                );
            }
        }
        Ok(())
    }
}

/// Converts a string to snake_case format.
///
/// Handles uppercase letters, spaces, and existing underscores to produce
/// a valid snake_case identifier (e.g., "CamelCaseString" → "camel_case_string").
///
/// # Arguments
///
/// * `input` - The input string to convert.
fn to_snake_case(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut prev_is_underscore = false;
    let mut prev_is_snaked = false;

    while let Some(ch) = chars.next() {
        match ch {
            // Handle uppercase letters
            'A'..='Z' => {
                if !result.is_empty() && !prev_is_underscore {
                    #[allow(clippy::if_same_then_else)]
                    if !prev_is_snaked {
                        result.push('_');
                    } else if chars.peek().is_some_and(|next| {
                        !next.is_uppercase()
                            && !next.is_ascii_digit()
                            && *next != '_'
                            && *next != ' '
                    }) {
                        result.push('_');
                    }
                }
                result.push(ch.to_ascii_lowercase());
                prev_is_underscore = false;
                prev_is_snaked = true;
            }
            // replace space by _
            ' ' => {
                result.push('_');
                prev_is_underscore = true;
                prev_is_snaked = false;
            }
            // Keep underscores as they are
            '_' => {
                result.push(ch);
                prev_is_underscore = true;
                prev_is_snaked = false;
            }
            // Keep other characters
            _ => {
                result.push(ch);
                prev_is_underscore = false;
                prev_is_snaked = false;
            }
        }
    }

    // Remove trailing underscore if present
    if result.ends_with('_') && result.len() > 1 {
        result.pop();
    }

    result
}

#[cfg(test)]
mod tests {

    use std::usize;

    use crate::{
        DateInputCodec, FieldName, Parser, Qualifiers,
        field::{ArrayField, Field},
        parse_date,
        timeline::TimeLineType,
    };

    use super::*;

    /// Tests that building a simple line with data and metadata produces the expected output.
    ///
    /// This test verifies:
    /// - That metadata is correctly inserted under the `ogre_md` field.
    /// - That fields from the input data are correctly extracted and included in the output.
    /// - That the structure of the resulting record matches expectations.
    #[test]
    fn simple_line() {
        let metadata = Metadata::new("test".into());

        let mut line_builder = LineBuilder::new(
            metadata,
            None,
            FieldMapping::new(vec![], None),
            true,
            false,
            false,
            true,
        );

        let mut data = Record::new();

        data.insert(
            "greetings".to_owned(),
            Value::String("Hello World".to_owned()),
        );
        data.insert("year".to_owned(), Value::Int(2025));

        line_builder.build(&mut data).unwrap();

        let record = &line_builder.line_data.data;

        // Verify flat fields are present
        match record.get("greetings").unwrap() {
            Value::String(val) => assert_eq!(*val, "Hello World"),
            _ => panic!("expected a String"),
        }

        match record.get("year").unwrap() {
            Value::Int(val) => assert_eq!(*val, 2025),
            _ => panic!("expected an Integer"),
        }
    }

    /// Tests that building a line with nested data structures works correctly.
    ///
    /// This test verifies:
    /// - That nested objects are correctly processed and included in the output.
    /// - That fields from deeply nested structures are correctly extracted.
    /// - That the structure of nested records matches expectations.
    #[test]
    fn nested_line() {
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            true,
            false,
            false,
            true,
        );
        let mut data = Record::new();

        data.insert(
            "greetings".to_owned(),
            Value::String("Hello World".to_owned()),
        );

        let mut level_2_data = Record::new();
        level_2_data.insert(
            "greetings".to_owned(),
            Value::String("Hello Lvl2".to_owned()),
        );

        let mut level_1_data = Record::new();

        level_1_data.insert("lvl2".to_owned(), Value::Object(level_2_data));
        level_1_data.insert(
            "greetings".to_owned(),
            Value::String("Hello Lvl1".to_owned()),
        );

        data.insert("lvl1".to_owned(), Value::Object(level_1_data));

        line_builder.build(&mut data).unwrap();

        let record = &line_builder.line_data.data;

        match record.get("greetings").unwrap() {
            Value::String(val) => assert_eq!(*val, "Hello World"),
            _ => panic!("expected a String"),
        }

        let lvl1 = match record.get("lvl1").unwrap() {
            Value::Object(val) => &val.0,
            _ => panic!("expected an Object"),
        };

        match lvl1.get("greetings").unwrap() {
            Value::String(val) => assert_eq!(*val, "Hello Lvl1"),
            _ => panic!("expected a String"),
        }

        let lvl2 = match lvl1.get("lvl2").unwrap() {
            Value::Object(val) => &val.0,
            _ => panic!("expected an Object"),
        };

        match lvl2.get("greetings").unwrap() {
            Value::String(val) => assert_eq!(*val, "Hello Lvl2"),
            _ => panic!("expected a String"),
        }
    }

    /// Tests that building a line with a field mapping produces the expected output.
    ///
    /// This test verifies:
    /// - That fields are correctly mapped according to the provided `FieldMapping`.
    /// - That qualifiers are correctly appended to field names when enabled.
    /// - That empty fields are included when `include_empty` is set to `true`.
    /// - That metadata is correctly inserted under the `ogre_md` field.
    #[test]
    fn simple_mapping() {
        let qualifiers = Qualifiers::new();
        let greetings = Field::Single {
            name: FieldName::new(
                "greetings".to_owned(),
                false,
                Some("output_greeting".to_owned()),
                Some(qualifiers.APP_NAME),
                None,
                Some("greetings desc".to_owned()),
            ),
            parser: Parser::String(),
            default_value: None,
        };

        let year = Field::Single {
            name: FieldName::new("year".to_owned(), false, None, None, None, None),
            parser: Parser::Int(),
            default_value: None,
        };

        let no_values = Field::Single {
            name: FieldName::new(
                "no_values".to_owned(),
                false,
                None,
                Some(qualifiers.APP_ID),
                None,
                None,
            ),
            parser: Parser::Int(),
            default_value: None,
        };

        let mapping = vec![greetings.clone(), year.clone(), no_values.clone()];
        let field_mapping = FieldMapping::new(mapping, None);

        let metadata = Metadata::new("test".into());

        let mut line_builder =
            LineBuilder::new(metadata, None, field_mapping, true, false, false, true);
        let mut data = Record::new();

        data.insert(
            greetings.output_name().to_owned(),
            Value::String("Hello World".to_owned()),
        );
        data.insert(year.output_name().to_owned(), Value::Int(2025));

        line_builder.build(&mut data).unwrap();

        let record = &line_builder.line_data.data;

        // Verify that the mapped field with qualifier is present
        assert!(record.contains_key("output_greeting:app_name"));
        // Verify that the simple field without qualifier is present
        assert!(record.contains_key("year"));
        // Verify that the field with app_id qualifier with null value is present
        assert!(record.contains_key("no_values:app_id"));
    }

    /// Tests that fields with an ignore parser are properly excluded even when the include_empty is set.
    #[test]
    fn ignore_parser_with_include_empty() {
        let greetings = Field::Single {
            name: FieldName::new("greetings".to_owned(), false, None, None, None, None),
            parser: Parser::String(),
            default_value: None,
        };

        let ignore = Field::Single {
            name: FieldName::new("ignore".to_owned(), false, None, None, None, None),
            parser: Parser::Ignore(),
            default_value: None,
        };

        let mapping = vec![greetings.clone(), ignore.clone()];
        let field_mapping = FieldMapping::new(mapping, None);

        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            field_mapping,
            true,
            false,
            false,
            true,
        );
        let mut data = Record::new();

        data.insert(
            greetings.output_name().to_owned(),
            Value::String("Hello World".to_owned()),
        );

        line_builder.build(&mut data).unwrap();

        let record = &line_builder.line_data.data;

        // Verify that the mapped field with qualifier is present
        assert!(record.contains_key("greetings"));

        // Verify that field with ignore parser is absent
        assert!(!record.contains_key("ignore"));
    }

    /// Tests that building a line with nested field mappings works correctly.
    ///
    /// This test verifies:
    /// - That nested objects are correctly processed and included in the output.
    /// - That fields from deeply nested structures are correctly extracted and mapped.
    /// - That qualifiers are correctly applied to nested fields.
    /// - That non-mapped fields are still included in the output.
    #[test]
    fn nested_mapping() {
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

        let level_2 = Field::Object {
            name: lvl2_name,
            ignore: false,
            fields: vec![level2_greetings.clone()],
        };

        let level_1_name = FieldName::new(
            "lvl1".to_owned(),
            false,
            Some("lvl1_output".to_owned()),
            Some(qualifiers.FILE_NAME),
            None,
            None,
        );

        let level_1 = Field::Object {
            name: level_1_name,
            ignore: false,
            fields: vec![level_2.clone()],
        };

        let greetings = Field::Single {
            name: FieldName::new(
                "greetings".to_owned(),
                false,
                Some("output_greeting".to_owned()),
                Some(qualifiers.CERT_SHA1),
                None,
                Some("greetings desc".to_owned()),
            ),
            parser: Parser::String(),
            default_value: None,
        };

        let mapping = vec![greetings.clone(), level_1.clone()];
        let field_mapping = FieldMapping::new(mapping, None);

        let metadata = Metadata::new("test".into());
        let mut line_builder =
            LineBuilder::new(metadata, None, field_mapping, true, false, false, true);
        let mut data = Record::new();

        data.insert(
            greetings.output_name().to_owned(),
            Value::String("Hello World".to_owned()),
        );

        let mut level_2_data = Record::new();
        level_2_data.insert(
            level2_greetings.output_name().to_owned(),
            Value::String("Hello Lvl2".to_owned()),
        );

        level_2_data.insert("l2_not_mapped".to_owned(), Value::Bool(true));

        let mut level_1_data = Record::new();

        level_1_data.insert(
            level_2.output_name().to_owned(),
            Value::Object(level_2_data),
        );

        level_1_data.insert("l1_not_mapped".to_owned(), Value::Bool(true));

        data.insert(
            level_1.output_name().to_owned(),
            Value::Object(level_1_data),
        );
        data.insert("not_mapped".to_owned(), Value::Bool(true));

        line_builder.build(&mut data).unwrap();

        let record = line_builder.line_data.data;

        // Verify top-level mapped field with qualifier
        assert!(record.contains_key("output_greeting:cert_sha1"));
        // Verify top-level non-mapped field is still included
        assert!(record.contains_key("not_mapped"));

        // Verify nested structure with correct qualifiers
        let lvl1 = match record.get("lvl1_output:file_name").unwrap() {
            Value::Object(val) => &val.0,
            _ => panic!("expected an Object"),
        };
        // Verify non-mapped field in nested object is included
        assert!(lvl1.contains_key("l1_not_mapped"));

        let lvl2 = match lvl1.get("lvl2_output:company_name").unwrap() {
            Value::Object(val) => &val.0,
            _ => panic!("expected an Object"),
        };

        // Verify deeply nested mapped field with qualifier
        assert!(lvl2.contains_key("lvl2_greeting:app_name"));
        // Verify non-mapped field in nested object is included
        assert!(lvl2.contains_key("l2_not_mapped"));
    }

    #[test]
    fn test_ignore_object() {
        let mut nested_tup = Record::new();

        nested_tup.add("nested_bool", Value::Bool(true));
        nested_tup.add("nested_str", Value::String("a nested".to_string()));

        let mut root = Record::new();
        root.add("str", Value::String("root".to_string()));
        root.add("array", Value::Array(vec![Value::Object(nested_tup)]));

        let mapping = vec![
            Field::Single {
                name: FieldName::new(
                    "str".to_owned(),
                    false,
                    Some("root_str".to_owned()),
                    None,
                    None,
                    None,
                ),
                parser: Parser::String(),
                default_value: None,
            },
            Field::Array(ArrayField::new(Field::Object {
                ignore: true,
                name: FieldName::new("array".to_owned(), false, None, None, None, None),
                fields: vec![],
            })),
        ];
        let field_mapping = FieldMapping::new(mapping, Some(Parser::String()));
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            field_mapping,
            true,
            false,
            false,
            true,
        );
        line_builder.build(&mut root).unwrap();
        let records = line_builder.line_data.data;
        assert_eq!(true, records.get("array").is_none());
    }

    #[test]
    fn integer_array() {
        let mut nested_tup = Record::new();

        nested_tup.add("nested_bool", Value::Bool(true));
        nested_tup.add("nested_str", Value::String("a nested".to_string()));

        let mut root = Record::new();
        root.add("str", Value::String("root".to_string()));
        root.add(
            "int_array",
            Value::Array(vec![Value::Int(0), Value::Int(1)]),
        );

        let qualifiers = Qualifiers::new();

        let mapping = vec![
            Field::Single {
                name: FieldName::new(
                    "str".to_owned(),
                    false,
                    Some("root_str".to_owned()),
                    None,
                    None,
                    None,
                ),
                parser: Parser::String(),
                default_value: None,
            },
            Field::Array(ArrayField::new(Field::Single {
                name: FieldName::new(
                    "ints".to_owned(),
                    false,
                    Some("int_array".to_owned()),
                    Some(qualifiers.APP_ID),
                    None,
                    None,
                ),
                parser: Parser::Int(),
                default_value: None,
            })),
        ];
        let field_mapping = FieldMapping::new(mapping, Some(Parser::String()));

        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            field_mapping,
            true,
            false,
            false,
            true,
        );

        line_builder.build(&mut root).unwrap();

        let array = line_builder.line_data.data.get("int_array:app_id").unwrap();

        match array {
            Value::Array(records) => {
                let val = &records[0];
                match val {
                    Value::Int(value) => {
                        assert_eq!(true, *value == 0 || *value == 1)
                    }
                    _ => panic!("should be an Int"),
                }
            }
            _ => panic!("should be an array"),
        }
    }

    #[test]
    fn nested_object_array() {
        let mut nested_tup = Record::new();

        nested_tup.add("nested_bool", Value::Bool(true));
        nested_tup.add("nested_str", Value::String("a nested".to_string()));

        let mut root = Record::new();
        root.add("str", Value::String("root".to_string()));
        root.add("array", Value::Array(vec![Value::Object(nested_tup)]));

        let mapping = vec![
            Field::Single {
                name: FieldName::new(
                    "str".to_owned(),
                    false,
                    Some("root_str".to_owned()),
                    None,
                    None,
                    None,
                ),
                parser: Parser::String(),
                default_value: None,
            },
            Field::Array(ArrayField::new(Field::Object {
                ignore: false,
                name: FieldName::new("array".to_owned(), false, None, None, None, None),
                fields: vec![],
            })),
        ];
        let field_mapping = FieldMapping::new(mapping, Some(Parser::String()));
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            field_mapping,
            false,
            false,
            false,
            false,
        );

        line_builder.build(&mut root).unwrap();
        let array = line_builder.line_data.data.get("array").unwrap();

        match array {
            Value::Array(records) => {
                let val = &records[0];
                match val {
                    Value::Object(record) => {
                        record.0.get("nested_bool").unwrap();
                    }
                    _ => panic!("should be an array"),
                }
            }
            _ => panic!("should be an array"),
        }
    }

    #[test]
    fn timeline_array() {
        let codec = DateInputCodec::from_str("iso");

        let mut nested_tup1 = Record::new();
        nested_tup1.add("comment", Value::String("first_comment".to_string()));
        let input = "1997-12-19T16:39:57.123456-08:00";
        let date = parse_date(input, &codec).unwrap();
        nested_tup1.add("timestamp", Value::Date(date));

        let mut nested_tup2 = Record::new();
        nested_tup2.add("comment", Value::String("second comment".to_string()));
        let input = "1997-12-19T16:39:57.123456-08:00";
        let date = parse_date(input, &codec).unwrap();
        nested_tup2.add("timestamp", Value::Date(date));

        let mut root = Record::new();
        root.add("message", Value::String("some message".to_string()));
        let input = "1995-12-19T16:39:57.123456-08:00";
        let date = parse_date(input, &codec).unwrap();
        root.add("timestamp", Value::Date(date));
        root.add(
            "comments",
            Value::Array(vec![Value::Object(nested_tup1), Value::Object(nested_tup2)]),
        );

        let qualifiers = Qualifiers::new();
        let mapping = vec![
            Field::Single {
                name: FieldName::new(
                    "timestamp".to_owned(),
                    false,
                    None,
                    None,
                    Some("Message Date".to_owned()),
                    None,
                ),
                parser: Parser::DateTime(DateInputCodec::Iso()),
                default_value: None,
            },
            Field::Array(ArrayField::new(Field::Object {
                ignore: false,
                name: FieldName::new("comments".to_owned(), false, None, None, None, None),
                fields: vec![
                    Field::Single {
                        name: FieldName::new(
                            "timestamp".to_owned(),
                            false,
                            None,
                            None,
                            Some("Comment Date".to_owned()),
                            None,
                        ),
                        parser: Parser::DateTime(DateInputCodec::Iso()),
                        default_value: None,
                    },
                    Field::Single {
                        name: FieldName::new(
                            "comment".to_owned(),
                            false,
                            None,
                            Some(qualifiers.APP_ID),
                            None,
                            None,
                        ),
                        parser: Parser::String(),
                        default_value: None,
                    },
                ],
            })),
        ];
        let field_mapping = FieldMapping::new(mapping, Some(Parser::String()));

        let mut timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "data_type".to_owned(),
            usize::MAX,
            None,
            None,
        );
        timeline_builder
            .add_description_ouput_path(vec!["comments".to_owned(), "comment".to_owned()]);

        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            Some(timeline_builder),
            field_mapping,
            false,
            false,
            false,
            true,
        );

        line_builder.build(&mut root).unwrap();
        assert_eq!(2, line_builder.line_data.timeline.len());

        for timeline in line_builder.line_data.timeline {
            if timeline
                .timestamp
                .to_rfc3339()
                .eq("1995-12-20T00:39:57.123456+00:00")
            {
                assert_eq!(timeline.timestamp_meaning, "Message Date");
            } else if timeline
                .timestamp
                .to_rfc3339()
                .eq("1997-12-20T00:39:57.123456+00:00")
            {
                assert_eq!(timeline.timestamp_meaning, "Comment Date");
            } else {
                panic!("unexpected timestamp: {}", timeline.timestamp);
            }
        }
    }

    #[test]
    fn nested_object_array_with_qualifiers() {
        let mut nested_tup = Record::new();

        nested_tup.add("nested_bool", Value::Bool(true));
        nested_tup.add("some_str", Value::String("a nested".to_string()));

        let mut root = Record::new();
        root.add("str", Value::String("root".to_string()));
        root.add("array", Value::Array(vec![Value::Object(nested_tup)]));

        let qualifiers = Qualifiers::new();

        let mapping = vec![
            Field::Single {
                name: FieldName::new(
                    "str".to_owned(),
                    false,
                    Some("root_str".to_owned()),
                    None,
                    None,
                    None,
                ),
                parser: Parser::String(),
                default_value: None,
            },
            Field::Array(ArrayField::new(Field::Object {
                name: FieldName::new(
                    "array".to_owned(),
                    false,
                    None,
                    Some(qualifiers.APP_CLSID),
                    None,
                    None,
                ),
                ignore: false,
                fields: vec![Field::Single {
                    name: FieldName::new(
                        "nested_str".to_owned(),
                        false,
                        Some("some_str".to_owned()),
                        Some(qualifiers.APP_ID),
                        None,
                        None,
                    ),
                    parser: Parser::String(),
                    default_value: None,
                }],
            })),
        ];
        let field_mapping = FieldMapping::new(mapping, Some(Parser::String()));

        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            field_mapping,
            true,
            false,
            false,
            true,
        );

        line_builder.build(&mut root).unwrap();
        let array = line_builder.line_data.data.get("array:app_clsid").unwrap();

        match array {
            Value::Array(records) => {
                let val = &records[0];
                match val {
                    Value::Object(record) => {
                        record.0.get("some_str:app_id").unwrap();
                    }
                    _ => panic!("should be an array"),
                }
            }
            _ => panic!("should be an array"),
        }
    }

    #[test]
    fn compute_hash_uses_only_primary_key_fields_when_present() {
        let field_mapping = primary_key_mapping();

        let first_id = build_hashed_line(
            Metadata::new("host".into()),
            field_mapping.clone(),
            "stable-id",
            "first message",
        );
        let second_id = build_hashed_line(
            Metadata::new("host".into()),
            field_mapping,
            "stable-id",
            "changed message",
        );

        assert_eq!(first_id, second_id);
    }

    #[test]
    fn compute_hash_includes_metadata_filename_and_vss() {
        let field_mapping = primary_key_mapping();
        let mut first_metadata = Metadata::new("host".into());
        first_metadata.original_filename = Some("artifact-a.evtx".to_owned());
        first_metadata.vss = Some("snapshot-1".to_owned());
        let mut second_metadata = Metadata::new("host".into());
        second_metadata.original_filename = Some("artifact-b.evtx".to_owned());
        second_metadata.vss = Some("snapshot-2".to_owned());

        let first_id = build_hashed_line(
            first_metadata,
            field_mapping.clone(),
            "stable-id",
            "same message",
        );
        let second_id =
            build_hashed_line(second_metadata, field_mapping, "stable-id", "same message");

        assert_ne!(first_id, second_id);
    }

    #[test]
    fn timeline_build_skips_unmapped_null_values() {
        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "test_data".to_owned(),
            usize::MAX,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            Some(timeline_builder),
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut record = Record::new();
        record.add("MissingValue", Value::Null());
        record.add("Message", Value::String("present".to_owned()));

        line_builder.build(&mut record).unwrap();

        assert!(!line_builder.line_data.data.contains_key("missing_value"));
        assert_eq!(
            line_builder.line_data.data.get("message"),
            Some(&Value::String("present".to_owned()))
        );
        assert!(line_builder.line_data.timeline.is_empty());
    }

    #[test]
    fn mapped_nested_object_does_not_invent_missing_child_fields() {
        let mapping = FieldMapping::new(
            vec![Field::Object {
                name: FieldName::new("details".to_owned(), false, None, None, None, None),
                ignore: false,
                fields: vec![
                    Field::Single {
                        name: FieldName::new("present".to_owned(), false, None, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    },
                    Field::Single {
                        name: FieldName::new("missing".to_owned(), false, None, None, None, None),
                        parser: Parser::String(),
                        default_value: None,
                    },
                ],
            }],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            mapping,
            false,
            false,
            false,
            false,
        );
        let mut details = Record::new();
        details.add("present", Value::String("kept".to_owned()));
        let mut record = Record::new();
        record.add("details", Value::Object(details));

        line_builder.build(&mut record).unwrap();

        let details = match line_builder.line_data.data.get("details").unwrap() {
            Value::Object(details) => details,
            value => panic!("expected details object, got {value:?}"),
        };
        assert_eq!(
            details.get("present"),
            Some(&Value::String("kept".to_owned()))
        );
        assert!(!details.contains_key("missing"));
    }

    #[test]
    fn ignored_object_mapping_skips_output() {
        let mapping = FieldMapping::new(
            vec![Field::Object {
                name: FieldName::new("details".to_owned(), false, None, None, None, None),
                ignore: true,
                fields: vec![Field::Single {
                    name: FieldName::new("hidden".to_owned(), false, None, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                }],
            }],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            mapping,
            false,
            false,
            false,
            false,
        );
        let mut details = Record::new();
        details.add("hidden", Value::String("secret".to_owned()));
        let mut record = Record::new();
        record.add("details", Value::Object(details));

        line_builder.build(&mut record).unwrap();

        assert!(line_builder.line_data.data.is_empty());
    }

    #[test]
    fn mapped_array_objects_preserve_renamed_and_unmapped_fields() {
        let qualifiers = Qualifiers::new();
        let mapping = FieldMapping::new(
            vec![Field::Array(ArrayField::new(Field::Object {
                name: FieldName::new("items".to_owned(), false, None, None, None, None),
                ignore: false,
                fields: vec![Field::Single {
                    name: FieldName::new(
                        "parsed_name".to_owned(),
                        false,
                        Some("item_name".to_owned()),
                        Some(qualifiers.APP_ID),
                        None,
                        None,
                    ),
                    parser: Parser::String(),
                    default_value: None,
                }],
            }))],
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            mapping,
            true,
            false,
            false,
            true,
        );
        let mut item = Record::new();
        item.add("item_name", Value::String("first".to_owned()));
        item.add("ExtraField", Value::Bool(true));
        let mut record = Record::new();
        record.add("items", Value::Array(vec![Value::Object(item)]));

        line_builder.build(&mut record).unwrap();

        let items = match line_builder.line_data.data.get("items").unwrap() {
            Value::Array(items) => items,
            value => panic!("expected items array, got {value:?}"),
        };
        let first = match &items[0] {
            Value::Object(first) => first,
            value => panic!("expected first item object, got {value:?}"),
        };
        assert_eq!(
            first.get("item_name:app_id"),
            Some(&Value::String("first".to_owned()))
        );
        assert_eq!(first.get("extra_field"), Some(&Value::Bool(true)));
    }

    #[test]
    fn unmapped_nested_objects_respect_force_snake_case() {
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut inner = Record::new();
        inner.add("InnerValue", Value::String("nested".to_owned()));
        let mut record = Record::new();
        record.add("OuterObject", Value::Object(inner));

        line_builder.build(&mut record).unwrap();

        let outer = match line_builder.line_data.data.get("outer_object").unwrap() {
            Value::Object(outer) => outer,
            value => panic!("expected outer object, got {value:?}"),
        };
        assert_eq!(
            outer.get("inner_value"),
            Some(&Value::String("nested".to_owned()))
        );
    }

    #[test]
    fn unmapped_null_is_preserved_without_timeline() {
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            None,
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut record = Record::new();
        record.add("MissingValue", Value::Null());

        line_builder.build(&mut record).unwrap();

        assert_eq!(
            line_builder.line_data.data.get("missing_value"),
            Some(&Value::Null())
        );
    }

    #[test]
    fn timeline_build_records_unmapped_date() {
        let codec = DateInputCodec::Iso();
        let date = parse_date("2020-01-01T00:00:00Z", &codec).unwrap();
        let timeline_builder = TimeLineBuilder::new(
            TimeLineType::Standard,
            "test_data".to_owned(),
            usize::MAX,
            None,
            None,
        );
        let mut line_builder = LineBuilder::new(
            Metadata::new("test".into()),
            Some(timeline_builder),
            FieldMapping::new(vec![], None),
            false,
            false,
            false,
            true,
        );
        let mut record = Record::new();
        record.add("UnmappedDate", Value::Date(date));

        line_builder.build(&mut record).unwrap();

        assert_eq!(line_builder.line_data.timeline.len(), 1);
        assert_eq!(
            line_builder.line_data.timeline[0].timestamp_meaning,
            "unmapped_date"
        );
        assert!(line_builder.line_data.data.contains_key("unmapped_date"));
        assert_eq!(line_builder.line_data.timeline[0].timestamp, date);
        assert_eq!(
            line_builder.line_data.data.get("unmapped_date"),
            Some(&Value::Date(date))
        );
    }

    fn primary_key_mapping() -> FieldMapping {
        FieldMapping::new(
            vec![
                Field::Single {
                    name: FieldName::new("id".to_owned(), true, None, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
                Field::Single {
                    name: FieldName::new("message".to_owned(), false, None, None, None, None),
                    parser: Parser::String(),
                    default_value: None,
                },
            ],
            None,
        )
    }

    fn build_hashed_line(
        metadata: Metadata,
        field_mapping: FieldMapping,
        id: &str,
        message: &str,
    ) -> String {
        let mut line_builder =
            LineBuilder::new(metadata, None, field_mapping, false, true, true, true);
        let mut record = Record::new();
        record.add("id", Value::String(id.to_owned()));
        record.add("message", Value::String(message.to_owned()));

        line_builder.build(&mut record).unwrap();

        line_builder.line_data.data_id.unwrap()
    }

    #[test]
    fn test_to_snake_case() {
        //empty
        assert_eq!(to_snake_case(""), "");

        //single character
        assert_eq!(to_snake_case("A"), "a");
        assert_eq!(to_snake_case("a"), "a");
        assert_eq!(to_snake_case("_"), "_");

        assert_eq!(to_snake_case("Hello"), "hello");
        assert_eq!(to_snake_case("XMLHttpRequest"), "xml_http_request");
        assert_eq!(to_snake_case("iPhone"), "i_phone");
        assert_eq!(to_snake_case("HTML"), "html");
        assert_eq!(to_snake_case("parseXML"), "parse_xml");
        assert_eq!(to_snake_case("parseJSONData"), "parse_json_data");

        assert_eq!(to_snake_case("Version2"), "version2");
        assert_eq!(to_snake_case("HTML5Parser"), "html5_parser");

        //Space
        assert_eq!(to_snake_case("Hello World"), "hello_world");
        assert_eq!(to_snake_case("OS Version"), "os_version");
        //existing underscore
        assert_eq!(to_snake_case("OS_Version"), "os_version");
        assert_eq!(to_snake_case("hello_World"), "hello_world");
        assert_eq!(to_snake_case("snake_case_test"), "snake_case_test");
        assert_eq!(to_snake_case("_Leading_underscore"), "_leading_underscore");
        assert_eq!(to_snake_case("trailing_Underscore_"), "trailing_underscore");
        assert_eq!(to_snake_case("EventID_attributes"), "event_id_attributes");
    }
}
