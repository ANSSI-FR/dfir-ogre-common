use crate::{Error, Value, date_util::DateOutputCodec};
use blake3::Hasher;

use indexmap::IndexMap;
use log::error;
use pyo3::prelude::*;
use std::fmt::Display;

/// Ordered map of field names to values.
/// `Record` wraps an `IndexMap`, keeping insertion order so JSON output stays deterministic.
/// It models a JSON object and is exposed to Python via `pyo3`. The struct stays lightweight,
/// holding `String` keys and `Value` values without extra baggage.
///

#[derive(Debug, Clone, Default, PartialEq)]
/// `Record` is an ordered collection of field names mapped to [`Value`]s.
/// Internally it uses an `IndexMap`, so insertion order is retained and JSON output is predictable.
/// The type is exposed to Python as a `pyo3` class; its methods simply forward to the `IndexMap`.
#[pyclass(from_py_object)]
pub struct Record(pub IndexMap<String, Value>);

impl Record {
    /// Creates an empty `Record` with the given capacity.
    pub fn with_capacity(size: usize) -> Self {
        Self(IndexMap::with_capacity(size))
    }

    /// Consumes entries, returning an iterator of `(String, Value)` pairs.
    pub fn drain(&mut self) -> indexmap::map::Drain<'_, String, Value> {
        self.0.drain(std::ops::RangeFull)
    }

    /// Extends the record with entries from an iterator of `(String, Value)` pairs.
    pub fn extend<I: IntoIterator<Item = (String, Value)>>(&mut self, iterable: I) {
        self.0.extend(iterable);
    }

    /// Returns an iterator over `(&String, &Value)` in insertion order.
    pub fn iter(&self) -> indexmap::map::Iter<'_, String, Value> {
        self.0.iter()
    }

    /// Looks up a field by name, yielding `Option<&Value>`.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// Hashes the entire record into the provided Blake3 hasher.
    ///
    /// The iteration respects insertion order, so two records with the same keys/values
    /// inserted in a different order will produce different hashes.
    ///
    /// # Arguments
    /// * `hasher` – A mutable reference to a `blake3::Hasher` that will be updated
    ///   with the record's contents.
    pub fn hash(&self, hasher: &mut Hasher) {
        for (key, value) in self.iter() {
            hasher.update(key.as_bytes());
            value.hash(hasher);
        }
    }

    /// Inserts a pair using an owned `String` key, avoiding an extra clone.
    pub fn insert(&mut self, name: String, value: Value) {
        self.0.insert(name, value);
    }
    /// Removes a key, returning its value if it existed.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.swap_remove(key)
    }

    /// Returns `true` if the field name exists in the record.
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn json_serialise(
        &self,
        buffer: &mut String,
        date_codec: &DateOutputCodec,
        include_empty: bool,
    ) -> Result<(), Error> {
        Value::json_serialise_record(self, buffer, date_codec, include_empty)
    }
}

#[pymethods]
impl Record {
    #[new]
    pub fn new() -> Self {
        Self(IndexMap::new())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Inserts a `(name, value)` pair, overwriting any existing entry.
    pub fn add(&mut self, name: &str, value: Value) {
        self.0.insert(name.to_string(), value);
    }

    /// Clears all entries but retains the allocated capacity.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Serializes the record to a JSON string using `Value::json_serialise_record`.
    pub fn to_string(&self) -> String {
        let mut buffer = String::new();
        if let Err(e) =
            Value::json_serialise_record(self, &mut buffer, &DateOutputCodec::Iso(), true)
        {
            error!("{e}");
            "".to_string()
        } else {
            buffer
        }
    }
}
impl Display for Record {
    /// Human‑readable JSON, identical to `to_string()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buffer = String::new();
        if let Err(e) =
            Value::json_serialise_record(self, &mut buffer, &DateOutputCodec::Iso(), true)
        {
            write!(f, "{e}")
        } else {
            f.write_str(&buffer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_add_and_get() {
        let mut tup = Record::new();
        tup.add("name", Value::String("Alice".into()));
        tup.insert("age".to_string(), Value::Int(30));
        tup.add("active", Value::Bool(true));

        assert_eq!(tup.get("name"), Some(&Value::String("Alice".into())));
        assert_eq!(tup.get("age"), Some(&Value::Int(30)));
        assert_eq!(tup.get("active"), Some(&Value::Bool(true)));
        assert!(tup.get("missing").is_none());
    }

    #[test]
    fn record_to_string_json_includes_all_fields() {
        let mut tup = Record::new();
        tup.add("null_val", Value::Null());
        tup.add("string", Value::String("Hello \"World\"\n".into()));
        tup.add("int", Value::Int(-42));
        tup.add("float", Value::Float(3.14));
        tup.add("bool", Value::Bool(false));

        let json = tup.to_string();
        // Expected JSON respects escaping and inclusion of null (since `include_empty` is true)
        let expected =
            r#"{"null_val":null,"string":"Hello \"World\"\n","int":-42,"float":3.14,"bool":false}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn value_string_escaping() {
        let special = "Line1\nLine2\r\t\"";
        let val = Value::String(special.to_string());

        let mut buf = String::new();
        Value::json_serialise_value(&val, &mut buf, &DateOutputCodec::Iso(), true).unwrap();

        // Verify that control characters are escaped as defined in `json_serialise_value`
        let expected = "\"Line1\\nLine2\\t\\\"\"";
        assert_eq!(buf, expected);
    }

    #[test]
    fn record_display_trait_outputs_json() {
        let mut tup = Record::new();
        tup.add("a", Value::Int(1));
        tup.add("b", Value::String("b".into()));

        let displayed = format!("{}", tup);
        let expected = r#"{"a":1,"b":"b"}"#;
        assert_eq!(displayed, expected);
    }

    #[test]
    fn record_equality() {
        // Build two identical records
        let mut t1 = Record::new();
        t1.add("id", Value::Int(1));
        t1.add("name", Value::String("Bob".into()));
        t1.add("active", Value::Bool(true));

        let mut t2 = Record::new();
        t2.add("id", Value::Int(1));
        t2.add("name", Value::String("Bob".into()));
        t2.add("active", Value::Bool(true));

        assert_eq!(t1, t2);
    }

    #[test]
    fn record_inequality() {
        // Record with a different key/value order
        let mut t1 = Record::new();
        t1.add("a", Value::Int(10));
        t1.add("b", Value::Int(20));

        let mut t2 = Record::new();
        t2.add("a", Value::Int(10));
        // Different value for key "b"
        t2.add("b", Value::Int(21));

        assert!(!(t1 == t2));
    }

    #[test]
    fn record_equality_with_nested_object() {
        // ---- build the inner record (object) ----
        let mut inner1 = Record::new();
        inner1.add("x", Value::Int(5));
        inner1.add("y", Value::String("y_val".into()));

        // ---- outer record 1 ----
        let mut outer1 = Record::new();
        outer1.add("id", Value::Int(100));
        outer1.add("payload", Value::Object(inner1.clone()));

        // ---- build an identical inner record for the second outer record ----
        let mut inner2 = Record::new();
        inner2.add("x", Value::Int(5));
        inner2.add("y", Value::String("y_val".into()));

        // ---- outer record 2 ----
        let mut outer2 = Record::new();
        outer2.add("id", Value::Int(100));
        outer2.add("payload", Value::Object(inner2));

        assert!(outer1 == outer2);
    }

    #[test]
    fn record_equality_with_array() {
        // ---- create an array value ----
        let array_val = Value::Array(vec![
            Value::Int(1),
            Value::String("two".into()),
            Value::Bool(false),
        ]);

        // ---- outer record 1 ----
        let mut tup1 = Record::new();
        tup1.add("id", Value::Int(42));
        tup1.add("data", array_val.clone());

        // ---- outer record 2 (identical) ----
        let mut tup2 = Record::new();
        tup2.add("id", Value::Int(42));
        tup2.add("data", array_val);

        // Equality should succeed – both the scalar field and the array field match
        assert_eq!(tup1, tup2);
        assert!(tup1 == tup2);
    }

    // ---------------------------------------------------------------------
    // Tests for `Record::hash`
    // ---------------------------------------------------------------------

    /// Helper that returns the Blake3 hash of a `Record`.
    fn record_hash(rec: &Record) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        rec.hash(&mut hasher);
        hasher.finalize()
    }

    #[test]
    fn record_hash_consistency_same_order() {
        let mut r1 = Record::new();
        r1.add("a", Value::Int(1));
        r1.add("b", Value::String("foo".into()));
        r1.add("c", Value::Bool(true));

        let mut r2 = Record::new();
        r2.add("a", Value::Int(1));
        r2.add("b", Value::String("foo".into()));
        r2.add("c", Value::Bool(true));

        assert_eq!(record_hash(&r1), record_hash(&r2));
    }

    #[test]
    fn record_hash_order_matters() {
        // Same key/value pairs inserted in different order should yield different hashes
        let mut r1 = Record::new();
        r1.add("a", Value::Int(1));
        r1.add("b", Value::Int(2));

        let mut r2 = Record::new();
        r2.add("b", Value::Int(2));
        r2.add("a", Value::Int(1));

        assert_ne!(record_hash(&r1), record_hash(&r2));
    }
    /// Helper that returns the Blake3 hash of a `Value`.
    fn hash_of(val: &Value) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        val.hash(&mut hasher);
        hasher.finalize()
    }
    #[test]
    fn value_hash_object_insertion_order_matters() {
        // Record with insertion order a -> b
        let mut rec1 = Record::new();
        rec1.add("a", Value::Int(1));
        rec1.add("b", Value::Int(2));
        let obj1 = Value::Object(rec1);

        // Same keys/values inserted in reverse order
        let mut rec2 = Record::new();
        rec2.add("b", Value::Int(2));
        rec2.add("a", Value::Int(1));
        let obj2 = Value::Object(rec2);

        assert_ne!(hash_of(&obj1), hash_of(&obj2));
    }
    #[test]
    fn value_csv_serialise_record() {
        // Build a record containing a mix of value types, including a Null which should
        // result in an empty field when CSV‑serialised
        let mut rec = Record::new();
        rec.add("name", Value::String("Alice".into()));
        rec.add("age", Value::Int(30));
        rec.add("missing", Value::Null());
        rec.add("active", Value::Bool(true));
        rec.add("array", Value::Array(vec![Value::Int(1), Value::Int(2)]));
        let mut object = Record::new();
        object.add("name", Value::String("Bob".into()));
        object.add("age", Value::Int(42));
        rec.add("object", Value::Object(object));

        // Serialise to CSV using a comma delimiter.
        let mut buf = String::new();
        Value::csv_serialise_record(&rec, &mut buf, &DateOutputCodec::Iso(), ',').unwrap();

        let expected = "\"Alice\",30,,true,\"[1,2]\",\"{\"\"name\"\":\"\"Bob\"\",\"\"age\"\":42}\"";
        assert_eq!(buf, expected);
    }
}
