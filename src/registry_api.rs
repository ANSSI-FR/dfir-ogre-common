use std::fmt::Display;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use crate::{
    Error, Record, Value,
    windows_utils::{SecurityDescriptor, from_filetime, security_descriptor_from_bytes},
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use log::error;
use pyo3::prelude::*;

use dfir_nt_hive::{
    Hive, KeyNode, KeyNodeItemRange, KeyValue, KeyValueData, KeyValueDataType, NtHiveError,
};
use pyo3::types::PyType;
use regex::Regex;
use zerocopy::SplitByteSlice;

#[pyclass]
/// Represents a registry hive loaded from a file.
///
/// The `Registry` holds the raw hive data and a normalized root name that
/// is used for all subsequent look‑ups. It is wrapped in an `Arc` because the
/// same hive can be shared across multiple `RegKey` instances.
pub struct Registry {
    hive_data: Arc<HiveData>,
    root_name: String,
}

impl Registry {
    /// Load a hive file and remember a normalised root name.
    ///
    /// Parses the file, stores the raw bytes in a shared `Arc`, and normalises
    /// the supplied root path to simplify later look‑ups.
    pub fn load_registry(input_file: &str, root_name: &str) -> Result<Registry, Error> {
        let hive_data = HiveData::new(input_file)?;

        Ok(Registry {
            hive_data: Arc::new(hive_data),
            root_name: normalize_path(root_name),
        })
    }

    /// Return a zero‑copy view over the stored hive bytes.
    ///
    /// The underlying `Hive` works directly on a byte slice, so this call is cheap.
    fn hive(&self) -> Result<Hive<&[u8]>, Error> {
        self.hive_data.hive()
    }
}

#[pymethods]
impl Registry {
    #[classmethod]
    ///
    /// Pyo3 Class method to load a registry from a file
    /// It is intended to be used only by the python binding
    /// # Arguments
    /// * `cls` - The class type
    /// * `input_file` - Path to the registry hive file
    /// * `root_name` - The root name to use for the registry
    ///
    pub fn load(
        _cls: &Bound<'_, PyType>,
        input_file: &str,
        root_name: &str,
    ) -> Result<Registry, Error> {
        Registry::load_registry(input_file, root_name)
    }

    ///
    /// Search for registry keys that match `path`.
    ///
    /// The function normalises `path`, checks it is under the configured `root_name`,
    /// walks the hive with a `KeySearcher`, and returns matching `RegKey` objects.
    pub fn glob_keys(&self, path: &str) -> Result<Vec<RegKey>, Error> {
        let path = normalize_path(path);
        let hive: Hive<&[u8]> = self.hive()?;

        if !path.starts_with(&self.root_name) {
            return Ok(vec![]);
        }
        let path = &path[self.root_name.len()..];
        let key_node = hive.root_key_node()?;

        let searcher = KeySearcher::new(path)?;
        let mut hive_keys = vec![];
        parse_sub_keys(
            &self.hive_data,
            key_node,
            self.root_name.clone(),
            &mut hive_keys,
            searcher,
        )?;

        Ok(hive_keys)
    }
}

#[pyclass]
/// Holds the raw bytes of a registry hive in a PyO3‑compatible struct.
/// Keeps the original file path for error reporting.
struct HiveData {
    input_file: String,
    hive_data: Vec<u8>,
}
impl HiveData {
    /// Creates a new HiveData instance from a file
    ///
    /// # Arguments
    /// * `input_file` - Path to the registry hive file
    fn new(input_file: &str) -> Result<Self, Error> {
        let mut hive_file = File::open(input_file)
            .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;

        let mut hive_data = Vec::with_capacity(hive_file.metadata().unwrap().len() as usize);
        hive_file.read_to_end(&mut hive_data).unwrap();
        Ok(Self {
            input_file: input_file.to_string(),
            hive_data,
        })
    }

    /// Build the hive from the stored data
    /// The hive object is implemented using zero_copy, it is very cheap to instanciate
    fn hive(&self) -> Result<Hive<&[u8]>, Error> {
        Hive::without_validation(self.hive_data.as_ref()).map_err(|e| {
            Error::NtHiveError(format!(
                "Error loading hive data: '{}' -  Error: {e}",
                self.input_file.to_owned()
            ))
        })
    }
}

#[derive(Clone)]
struct RegValues(IndexMap<String, RegValue>);

#[derive(Clone)]
#[pyclass(from_py_object)]
/// The key holds a reference to the shared `HiveData`, its position in the hive,
/// timestamps, security descriptor and the collection of values it contains.
pub struct RegKey {
    hive: Arc<HiveData>,
    key_range: KeyNodeItemRange,
    #[pyo3(get)]
    mtime: DateTime<Utc>,
    #[pyo3(get)]
    security_descriptor: SecurityDescriptor,
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    name: String,
    values: RegValues,
}
impl RegKey {
    /// Creates a RegKey instance from a KeyNode
    ///
    /// # Arguments
    /// * `hive` - The HiveData instance
    /// * `node` - The KeyNode to create the RegKey from
    /// * `path` - The full path of the key
    fn from<B>(hive: Arc<HiveData>, node: &KeyNode<'_, B>, path: &str) -> Result<Self, Error>
    where
        B: SplitByteSlice,
    {
        let key_name = node.name()?.to_string_lossy();

        let filetime = from_filetime(node.timestamp());
        let security_descriptor_b = node.security_descriptor()?;
        let path = format!("{path}\\{key_name}");
        let key_security = security_descriptor_from_bytes(&security_descriptor_b)?;

        let mut hive_key = RegKey {
            hive: hive.clone(),
            key_range: node.item_range.clone(),
            mtime: filetime,
            security_descriptor: key_security,
            path: path.to_string(),
            name: key_name,
            values: RegValues(IndexMap::new()),
        };

        // Populate values
        if let Some(value_iter) = node.values() {
            let value_iter = value_iter?;
            for value in value_iter {
                let mut reg_value = RegValue {
                    ..Default::default()
                };
                if let Err(error) = parse_key_value(value, &mut reg_value) {
                    reg_value.error = Some(error.to_string());
                    error!("Error while parsing value for key {path}. {error}")
                }
                hive_key.values.0.insert(reg_value.name(), reg_value);
            }
        }
        Ok(hive_key)
    }
}

#[pymethods]
impl RegKey {
    /// Gets every sub keys of this registry key
    pub fn sub_keys(&self) -> Result<Vec<RegKey>, Error> {
        let hive = self.hive.hive()?;
        let node: KeyNode<'_, &[u8]> = KeyNode {
            hive: &hive,
            item_range: self.key_range.clone(),
        };
        let mut result = vec![];
        let subkeys = node.subkeys();
        if let Some(sub_keys) = subkeys {
            for key_node in sub_keys? {
                let key_node = key_node?;

                let reg_key = RegKey::from(self.hive.clone(), &key_node, &self.path)?;
                result.push(reg_key);
            }
        }

        Ok(result)
    }

    /// Gets a specific subkey by name
    pub fn sub_key(&self, path: &str) -> Result<Option<RegKey>, Error> {
        let hive = self.hive.hive()?;
        let node: KeyNode<'_, &[u8]> = KeyNode {
            hive: &hive,
            item_range: self.key_range.clone(),
        };
        if let Some(sub_key) = node.subkey(path) {
            let sub_key = sub_key?;
            let reg_key = RegKey::from(self.hive.clone(), &sub_key, &self.path)?;
            return Ok(Some(reg_key));
        }
        Ok(None)
    }

    /// Gets a specific subkey by path
    pub fn sub_path(&self, path: &str) -> Result<Option<RegKey>, Error> {
        let mut res = self.sub_glob(path)?;
        //return the last found key
        Ok(res.pop())
    }

    /// Gets subkeys that match a glob pattern
    pub fn sub_glob(&self, path: &str) -> Result<Vec<RegKey>, Error> {
        let hive: Hive<&[u8]> = self.hive.hive()?;
        let node: KeyNode<'_, &[u8]> = KeyNode {
            hive: &hive,
            item_range: self.key_range.clone(),
        };

        let searcher = KeySearcher::new(path)?;
        let mut hive_keys = vec![];
        parse_sub_keys(
            &self.hive,
            node,
            self.path.clone(),
            &mut hive_keys,
            searcher,
        )?;

        Ok(hive_keys)
    }

    /// Gets a specific value by name
    pub fn value(&self, name: &str) -> Option<RegValue> {
        self.values.0.get(name).cloned()
    }

    /// Gets a specific value by name
    pub fn values(&self) -> Vec<RegValue> {
        self.values.0.values().cloned().collect()
    }

    /// Gets the data of a specific value
    #[pyo3(signature = (name, default=None))]
    pub fn value_data(
        &self,
        name: &str,
        default: Option<Py<PyAny>>,
    ) -> Result<Option<Py<PyAny>>, Error> {
        if let Some(value) = self.value(name) {
            let data = value.data()?;
            Ok(Some(data))
        } else {
            Ok(default)
        }
    }

    /// Converts the registry key to a record representation
    pub fn to_record(&self) -> Record {
        let mut record = Record::with_capacity(5);
        record.add("name", Value::String(self.name.clone()));
        record.add("path", Value::String(self.path.clone()));
        record.add("mtime", Value::Date(self.mtime));
        let mut values: Vec<Value> = Vec::with_capacity(self.values.0.len());
        for value in self.values.0.values() {
            values.push(Value::Object(value.to_record()));
        }
        record.add("values", Value::Array(values));
        record.add(
            "security_descriptor",
            Value::Object(self.security_descriptor.to_record()),
        );
        record
    }
}

/// Represents a registry value and its data
#[derive(Debug, Clone, Default)]
#[pyclass(from_py_object)]
/// Holds a single registry value and its parsed representation.
///
/// Depending on the underlying type, only one of the data fields (`bin_data`,
/// `string_data`, `string_array_data`, `int_data`) will be populated.
pub struct RegValue {
    name: String,
    data_type: DataType,
    bin_data: Option<Vec<u8>>,
    string_data: Option<String>,
    string_array_data: Option<Vec<String>>,
    int_data: Option<i64>,
    valid_signature: bool,
    error: Option<String>,
}
#[pymethods]
impl RegValue {
    /// Returns the name of the registry value
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Returns the data type of the registry value as a string
    pub fn r#type(&self) -> String {
        self.data_type.to_string()
    }

    /// Gets the data of the registry value as a Python object
    pub fn data(&self) -> Result<Py<PyAny>, Error> {
        if let Some(value) = &self.string_data {
            let data: PyResult<Py<PyAny>> = Python::attach(|py| -> PyResult<_> {
                let data: Bound<'_, PyAny> = value.into_pyobject(py)?.into_any();
                Ok(data.into())
            });

            let data = data?;
            Ok(data)
        } else if let Some(value) = &self.int_data {
            let data: PyResult<Py<PyAny>> = Python::attach(|py| -> PyResult<_> {
                let data = value.into_pyobject(py)?;
                Ok(data.into())
            });

            let data = data?;
            Ok(data)
        } else if let Some(value) = &self.string_array_data {
            let data: PyResult<Py<PyAny>> = Python::attach(|py| -> PyResult<_> {
                let data: Bound<'_, PyAny> = value.into_pyobject(py)?.into_any();

                Ok(data.into())
            });
            let data = data?;
            Ok(data)
        } else if let Some(value) = &self.bin_data {
            let binary: PyResult<Py<PyAny>> = Python::attach(|py| -> PyResult<_> {
                let data = value.clone().into_pyobject(py)?;
                Ok(data.into())
            });
            let bin = binary?;
            Ok(bin)
        } else {
            let binary: PyResult<Py<PyAny>> = Python::attach(|py| -> PyResult<_> {
                let data = None::<String>.into_pyobject(py)?;
                Ok(data.into())
            });
            let bin = binary?;
            Ok(bin)
        }
    }

    /// Converts the registry value to a record representation
    fn to_record(&self) -> Record {
        let mut record = Record::with_capacity(6);
        record.add("name", Value::String(self.name.to_string()));
        record.add("type", Value::String(self.data_type.to_string()));

        const DATA_FIELD: &str = "data";
        if let Some(data) = &self.string_data {
            record.add(DATA_FIELD, Value::String(data.clone()));
        } else if let Some(data) = &self.string_array_data {
            let field: Vec<Value> = data.iter().map(|v| Value::String(v.to_string())).collect();
            record.add(DATA_FIELD, Value::Array(field));
        } else if let Some(data) = &self.int_data {
            record.add(DATA_FIELD, Value::Int(*data));
        } else if let Some(bin_data) = &self.bin_data
            && !bin_data.is_empty()
        {
            record.add(
                DATA_FIELD,
                Value::String(format!("0x{}", hex::encode(bin_data))),
            );
        }

        // Add validity information if needed
        if !self.valid_signature {
            record.add("valid_signature", Value::Bool(false));
        }
        // Add error information if present
        if let Some(error) = &self.error {
            record.add("error", Value::String(error.to_string()));
        }

        record
    }
}

#[derive(Debug, Clone, Default)]
#[pyclass(from_py_object)]
#[allow(non_camel_case_types)]
/// Enumerates the possible Windows registry value types.
///
/// The variants correspond directly to the `REG_*` constants used by the
/// Windows API.
pub enum DataType {
    #[default]
    REG_FIRST_INVALID,
    REG_SZ,
    REG_EXPAND_SZ,
    REG_BINARY,
    REG_DWORD_LE,
    REG_DWORD_BE,
    REG_LINK,
    REG_MULTI_SZ,
    REG_RESOURCE_LIST,
    REG_FULL_RESOURCE_DESCRIPTOR,
    REG_RESOURCE_REQUIREMENT_LIST,
    REG_QWORD_LE,
}
impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let to_str = match self {
            DataType::REG_FIRST_INVALID => "REG_FIRST_INVALID",
            DataType::REG_SZ => "REG_SZ",
            DataType::REG_EXPAND_SZ => "REG_EXPAND_SZ",
            DataType::REG_BINARY => "REG_BINARY",
            DataType::REG_DWORD_LE => "REG_DWORD_LE",
            DataType::REG_DWORD_BE => "REG_DWORD_BE",
            DataType::REG_LINK => "REG_LINK",
            DataType::REG_MULTI_SZ => "REG_MULTI_SZ",
            DataType::REG_RESOURCE_LIST => "REG_RESOURCE_LIST",
            DataType::REG_FULL_RESOURCE_DESCRIPTOR => "REG_FULL_RESOURCE_DESCRIPTOR",
            DataType::REG_RESOURCE_REQUIREMENT_LIST => "REG_RESOURCE_REQUIREMENT_LIST",
            DataType::REG_QWORD_LE => "REG_QWORD_LE",
        };
        f.write_str(to_str)
    }
}

#[derive(Debug, Clone)]
/// Incremental matcher for registry key paths.
///
/// It stores a sequence of search tokens (exact, glob, or any) and a list of
/// candidate positions that remain viable as the search proceeds.
struct KeySearcher {
    search_keys: Vec<KeySearchType>,
    candidate: Vec<usize>,
}
impl KeySearcher {
    pub fn new(search: &str) -> Result<Self, Error> {
        let mut search_keys = vec![];
        for key in search.split("\\") {
            if key.is_empty() {
                continue;
            }
            if key.contains("**") {
                search_keys.push(KeySearchType::Any);
            } else if key.contains("*") {
                let reg_exp = Regex::new(&key.replace("*", ".*"))?;
                search_keys.push(KeySearchType::Glob(reg_exp));
            } else {
                search_keys.push(KeySearchType::Equals(key.to_string()));
            }
        }
        let candidate = vec![0];

        Ok(Self {
            search_keys,
            candidate,
        })
    }

    pub fn match_key(&mut self, key: &str) -> MatchStatus {
        let mut surviving = vec![];

        let mut final_status = MatchStatus::InProgress;

        for pos in &self.candidate {
            if *pos < self.search_keys.len() {
                let search_item = &self.search_keys[*pos];

                let status = match search_item {
                    KeySearchType::Equals(value) => {
                        self.key_equals(key, &mut surviving, *pos, value)
                    }
                    KeySearchType::Glob(regex) => self.key_match(key, &mut surviving, *pos, regex),
                    KeySearchType::Any => {
                        let next_pos = pos + 1;
                        if next_pos == self.search_keys.len() {
                            surviving.push(*pos);
                            MatchStatus::True
                        } else {
                            let search_item = &self.search_keys[next_pos];
                            let status = match search_item {
                                KeySearchType::Equals(value) => {
                                    self.key_equals(key, &mut surviving, next_pos, value)
                                }
                                KeySearchType::Glob(regex) => {
                                    self.key_match(key, &mut surviving, next_pos, regex)
                                }
                                KeySearchType::Any => {
                                    surviving.push(next_pos);
                                    MatchStatus::InProgress
                                }
                            };

                            surviving.push(*pos);

                            status
                        }
                    }
                };

                if let MatchStatus::True = status {
                    final_status = status;
                }
            }
        }

        self.candidate = surviving;

        if MatchStatus::True == final_status {
            MatchStatus::True
        } else if self.candidate.is_empty() {
            MatchStatus::False
        } else {
            final_status
        }
    }

    fn key_match(
        &self,
        key: &str,
        surviving: &mut Vec<usize>,
        pos: usize,
        regex: &Regex,
    ) -> MatchStatus {
        if regex.is_match(key) {
            let new_pos = pos + 1;
            if new_pos == self.search_keys.len() {
                MatchStatus::True
            } else {
                surviving.push(new_pos);
                MatchStatus::InProgress
            }
        } else {
            MatchStatus::False
        }
    }

    fn key_equals(
        &self,
        key: &str,
        surviving: &mut Vec<usize>,
        pos: usize,
        value: &String,
    ) -> MatchStatus {
        if value.eq(key) {
            let new_pos = pos + 1;
            if new_pos == self.search_keys.len() {
                MatchStatus::True
            } else {
                surviving.push(new_pos);
                MatchStatus::InProgress
            }
        } else {
            MatchStatus::False
        }
    }
}

/// Remove a leading back‑slash from a registry path, if present.
fn normalize_path(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("\\") {
        stripped.to_string()
    } else {
        path.to_string()
    }
}

/// Recursively walk a hive subtree, collecting keys that match `searcher`.
fn parse_sub_keys<B>(
    hive_data: &Arc<HiveData>,
    key_node: KeyNode<B>,
    path: String,
    key_output: &mut Vec<RegKey>,
    searcher: KeySearcher,
) -> Result<(), Error>
where
    B: SplitByteSlice,
{
    if let Some(subkeys) = key_node.subkeys() {
        let subkeys = subkeys?;

        for key_node in subkeys {
            let mut searcher = searcher.clone();
            let key_node = key_node?;

            let key_name = key_node.name()?.to_string_lossy();

            match searcher.match_key(&key_name) {
                MatchStatus::InProgress => {
                    let path = format!("{path}\\{key_name}");
                    parse_sub_keys(hive_data, key_node, path, key_output, searcher)?;
                }
                MatchStatus::False => {}
                MatchStatus::True => {
                    let key = RegKey::from(hive_data.clone(), &key_node, &path)?;
                    key_output.push(key)
                }
            }

            //parse_sub_keys(hive_data, key_node, &path, key_output, searcher)?;
        }
    }

    Ok(())
}

fn parse_key_value<B>(
    value: Result<KeyValue<'_, B>, NtHiveError>,
    hive_value: &mut RegValue,
) -> Result<(), Error>
where
    B: SplitByteSlice,
{
    let value: KeyValue<'_, B> = value?;

    let mut value_name = value.name()?.to_string_lossy();

    if value_name.is_empty() {
        value_name.push_str("(default)");
    }

    hive_value.name = value_name;
    hive_value.valid_signature = value.validate_signature()?;

    match value.data_type() {
        Ok(value_type) => match value_type {
            KeyValueDataType::RegSZ => {
                let string_data = value.string_data()?;
                hive_value.data_type = DataType::REG_SZ;
                hive_value.string_data = Some(string_data);
            }

            KeyValueDataType::RegExpandSZ => {
                hive_value.data_type = DataType::REG_EXPAND_SZ;
                let string_data = value.string_data()?;
                hive_value.string_data = Some(string_data);
            }
            KeyValueDataType::RegBinary => {
                hive_value.data_type = DataType::REG_BINARY;
                hive_value.bin_data = Some(get_bin_value(&value)?);
            }
            KeyValueDataType::RegDWord => {
                hive_value.data_type = DataType::REG_DWORD_LE;
                hive_value.int_data = Some(value.dword_data()? as i64);
            }

            KeyValueDataType::RegDWordBigEndian => {
                hive_value.data_type = DataType::REG_DWORD_BE;
                hive_value.int_data = Some(value.dword_data()? as i64);
            }
            KeyValueDataType::RegMultiSZ => {
                hive_value.data_type = DataType::REG_MULTI_SZ;
                let multi_string_data = value
                    .multi_string_data()?
                    .collect::<Result<Vec<_>, NtHiveError>>()?;
                hive_value.string_array_data = Some(multi_string_data);
            }
            KeyValueDataType::RegQWord => {
                hive_value.data_type = DataType::REG_QWORD_LE;
                hive_value.int_data = Some(value.qword_data()? as i64);
            }
            KeyValueDataType::RegNone => {
                hive_value.data_type = DataType::REG_FIRST_INVALID;
                hive_value.bin_data = Some(get_bin_value(&value)?);
            }
            KeyValueDataType::RegLink => {
                hive_value.data_type = DataType::REG_LINK;
                hive_value.bin_data = Some(get_bin_value(&value)?);
            }
            KeyValueDataType::RegResourceList => {
                hive_value.data_type = DataType::REG_RESOURCE_LIST;
                hive_value.bin_data = Some(get_bin_value(&value)?);
            }
            KeyValueDataType::RegFullResourceDescriptor => {
                hive_value.data_type = DataType::REG_FULL_RESOURCE_DESCRIPTOR;
                hive_value.bin_data = Some(get_bin_value(&value)?);
            }
            KeyValueDataType::RegResourceRequirementsList => {
                hive_value.data_type = DataType::REG_RESOURCE_REQUIREMENT_LIST;
                hive_value.bin_data = Some(get_bin_value(&value)?);
            }
        },

        Err(e) => {
            hive_value.data_type = DataType::REG_FIRST_INVALID;
            hive_value.bin_data = Some(get_bin_value(&value)?);
            hive_value.error = Some(e.to_string())
        }
    };

    Ok(())
}

fn get_bin_value<B>(value: &KeyValue<'_, B>) -> Result<Vec<u8>, Error>
where
    B: SplitByteSlice,
{
    let binary_data = value.data()?;
    Ok(match binary_data {
        KeyValueData::Small(data) => data.to_vec(),
        KeyValueData::Big(iter) => {
            let data = iter.collect::<Result<Vec<_>, NtHiveError>>()?;

            let data_size = value.data_size();
            let mut value: Vec<u8> = Vec::with_capacity(data_size as usize);
            for slice in data {
                value.extend_from_slice(slice);
            }

            value
        }
    })
}

#[derive(Debug, Clone)]
/// Token used by `KeySearcher` to describe each component of the search pattern.
enum KeySearchType {
    /// Exact string match.
    Equals(String),
    Glob(Regex),
    Any,
}

#[derive(PartialEq, Debug)]
/// Result of a single `KeySearcher::match_key` call.
///
/// `InProgress` means more components are needed; `True` means the full pattern
/// matched; `False` indicates a mismatch.
enum MatchStatus {
    InProgress,
    False,
    True,
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn searcher_glob_search() {
        let search = KeySearcher::new("\\test\\").unwrap();
        assert_eq!(1, search.search_keys.len());
        assert_eq!(MatchStatus::True, search.clone().match_key("test"));
        assert_eq!(MatchStatus::False, search.clone().match_key("test2"));

        let search = KeySearcher::new("\\*\\").unwrap();

        assert_eq!(MatchStatus::True, search.clone().match_key("test"));
        assert_eq!(MatchStatus::True, search.clone().match_key("test2"));

        let mut search = KeySearcher::new("\\test\\*\\start*middle*end\\").unwrap();
        assert_eq!(3, search.search_keys.len());
        assert_eq!(MatchStatus::False, search.clone().match_key("test2"));
        assert_eq!(MatchStatus::InProgress, search.match_key("test"));
        assert_eq!(MatchStatus::InProgress, search.match_key("anything"));

        assert_eq!(MatchStatus::False, search.clone().match_key("anything"));
        assert_eq!(
            MatchStatus::True,
            search.clone().match_key("startmiddleend")
        );

        assert_eq!(
            MatchStatus::True,
            search.clone().match_key("start_middle_anything_end")
        );
    }

    #[test]
    fn searcher_any_search() {
        let mut search = KeySearcher::new("\\**\\").unwrap();
        assert_eq!(1, search.search_keys.len());

        assert_eq!(MatchStatus::True, search.match_key("test"));
        assert_eq!(MatchStatus::True, search.match_key("test2"));
        assert_eq!(MatchStatus::True, search.match_key("test3"));
        assert_eq!(MatchStatus::True, search.match_key("test4"));

        let mut search = KeySearcher::new("\\**\\test*").unwrap();
        assert_eq!(2, search.search_keys.len());

        assert_eq!(MatchStatus::InProgress, search.match_key("some"));
        assert_eq!(MatchStatus::InProgress, search.match_key("other"));
        assert_eq!(MatchStatus::True, search.match_key("test1"));
        assert_eq!(MatchStatus::InProgress, search.match_key("sesre"));
        assert_eq!(MatchStatus::True, search.match_key("test1"));

        let mut search = KeySearcher::new("test\\**\\test*\\END").unwrap();
        assert_eq!(MatchStatus::InProgress, search.match_key("test"));
        assert_eq!(MatchStatus::InProgress, search.match_key("some"));
        assert_eq!(MatchStatus::InProgress, search.match_key("other"));
        assert_eq!(MatchStatus::InProgress, search.match_key("test245"));
        assert_eq!(MatchStatus::True, search.match_key("END"));
    }

    #[test]
    fn registry_search() {
        let registry = Registry::load_registry("test_data/hive/testhive", "\\test\\data").unwrap();

        let results = registry.glob_keys("\\test\\data").unwrap();
        assert_eq!(0, results.len());

        let results = registry.glob_keys("\\test\\data\\*").unwrap();
        assert_eq!(5, results.len());
        assert_eq!("big-data-test", results[0].name);
        assert_eq!("subpath-test", results[4].name);

        let results = registry.glob_keys("\\test\\data\\*\\").unwrap();
        assert_eq!(5, results.len());
        assert_eq!("big-data-test", results[0].name);
        assert_eq!("subpath-test", results[4].name);
        let big_data_value = &results[0].values.0;

        assert_eq!(
            16343,
            big_data_value
                .get("A")
                .unwrap()
                .bin_data
                .clone()
                .unwrap()
                .len()
        );
        assert_eq!(
            16344,
            big_data_value
                .get("B")
                .unwrap()
                .bin_data
                .clone()
                .unwrap()
                .len()
        );
        assert_eq!(
            16345,
            big_data_value
                .get("C")
                .unwrap()
                .bin_data
                .clone()
                .unwrap()
                .len()
        );

        let results = registry.glob_keys("\\test\\data\\*path*").unwrap();
        assert_eq!(1, results.len());
        assert_eq!("subpath-test", results[0].name);

        let results = registry.glob_keys("\\test\\data\\*path*\\*").unwrap();
        assert_eq!(3, results.len());
        assert_eq!("no-subkeys", results[0].name);
        assert_eq!("with-single-level-subkey", results[1].name);
        assert_eq!("with-two-levels-of-subkeys", results[2].name);
    }

    #[test]
    fn key_subkeys() {
        let registry = Registry::load_registry("test_data/hive/testhive", "\\test\\data").unwrap();

        let results = registry.glob_keys("\\test\\data").unwrap();
        assert_eq!(0, results.len());

        let results = registry.glob_keys("\\test\\data\\*path*").unwrap();
        assert_eq!(1, results.len());
        let key = &results[0];
        assert_eq!("subpath-test", &key.name);

        let results = key.sub_keys().unwrap();
        assert_eq!(3, results.len());
        assert_eq!("no-subkeys", results[0].name);
        assert_eq!("with-single-level-subkey", results[1].name);
        assert_eq!("with-two-levels-of-subkeys", results[2].name);

        let two_levels = &results[2];
        let results = two_levels.sub_keys().unwrap();
        assert_eq!(1, results.len());

        let one_more = &results[0];
        assert_eq!(
            "test\\data\\subpath-test\\with-two-levels-of-subkeys\\subkey1",
            one_more.path
        );

        let results = one_more.sub_keys().unwrap();
        assert_eq!(1, results.len());
        let last_one = &results[0];
        assert_eq!(
            "test\\data\\subpath-test\\with-two-levels-of-subkeys\\subkey1\\subkey2",
            last_one.path
        );
        let results = last_one.sub_keys().unwrap();
        assert_eq!(0, results.len());
    }

    #[test]
    fn key_subkey() {
        let registry = Registry::load_registry("test_data/hive/testhive", "\\test\\data").unwrap();

        let results = registry.glob_keys("\\test\\data").unwrap();
        assert_eq!(0, results.len());

        let results = registry.glob_keys("\\test\\data\\*path*").unwrap();
        assert_eq!(1, results.len());
        let key = &results[0];
        assert_eq!("subpath-test", &key.name);

        let results = key.sub_key("with-single-level-subkey").unwrap().unwrap();
        assert_eq!(
            "test\\data\\subpath-test\\with-single-level-subkey",
            results.path
        );
        assert_eq!("with-single-level-subkey", results.name);

        let results = key.sub_key("with-two-levels-of-subkeys").unwrap().unwrap();
        assert_eq!(
            "test\\data\\subpath-test\\with-two-levels-of-subkeys",
            results.path
        );
        assert_eq!("with-two-levels-of-subkeys", results.name);

        let results = key.sub_key("with-two-levels-of-subkeys\\subkey1").unwrap();

        assert_eq!(true, results.is_none());
    }

    #[test]
    fn sub_path_glob() {
        let registry = Registry::load_registry("test_data/hive/testhive", "\\test\\data").unwrap();

        let results = registry.glob_keys("\\test\\data").unwrap();
        assert_eq!(0, results.len());

        let results = registry.glob_keys("\\test\\data\\*path*").unwrap();
        assert_eq!(1, results.len());
        let key = &results[0];
        assert_eq!("subpath-test", &key.name);
        assert_eq!("test\\data\\subpath-test", &key.path);

        let results = key.sub_key("with-two-levels-of-subkeys\\subkey1").unwrap();
        assert_eq!(true, results.is_none());

        let results = key.sub_path("with-two-levels-of-subkeys\\subkey1").unwrap();
        assert_eq!(true, results.is_some());
        let results = results.unwrap();
        assert_eq!(
            "test\\data\\subpath-test\\with-two-levels-of-subkeys\\subkey1",
            results.path
        );

        let results = key.sub_glob("with-two-levels-of-subkeys\\subkey1").unwrap();
        assert_eq!(1, results.len());
        let results = &results[0];
        assert_eq!(
            "test\\data\\subpath-test\\with-two-levels-of-subkeys\\subkey1",
            results.path
        );
    }

    #[test]
    fn value_data() {
        let registry = Registry::load_registry("test_data/hive/testhive", "").unwrap();

        let mut results = registry.glob_keys("\\data-test").unwrap();
        assert_eq!(1, results.len());
        let key = results.pop().unwrap();
        assert_eq!(true, key.value("not_exist").is_none());
        let value = key.value("reg-sz");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        assert_eq!("sz-test", value.string_data.unwrap());

        let value = key.value("reg-expand-sz");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        assert_eq!("sz-test", value.string_data.unwrap());

        let value = key.value("reg-sz-with-terminating-nul");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        assert_eq!("sz-test", value.string_data.unwrap());

        let value = key.value("reg-multi-sz");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        let data = value.string_array_data.unwrap();
        assert_eq!("multi-sz-test", &data[0]);
        assert_eq!("line2", &data[1]);

        let value = key.value("reg-multi-sz-big");
        assert_eq!(true, value.is_some());
        let value = value.unwrap().string_array_data.unwrap().join("");
        assert_eq!(
            "01234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789",
            value
        );

        let value = key.value("dword");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        assert_eq!(42, value.int_data.unwrap());

        let value = key.value("dword-big-endian");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        assert_eq!(704643072, value.int_data.unwrap());

        let value = key.value("qword");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        let int_value: u64 = 18446744073709551615;
        assert_eq!(int_value as i64, value.int_data.unwrap());

        let value = key.value("binary");
        assert_eq!(true, value.is_some());
        let value = value.unwrap();
        let bin_data = value.bin_data.unwrap();
        assert_eq!(5, bin_data.len());

        assert_eq!("0102030405", hex::encode(&bin_data))
    }
}
