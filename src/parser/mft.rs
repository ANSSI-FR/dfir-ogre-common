//! Parser for a raw NTFS `$MFT`, used when the collection holds the Master File
//! Table itself instead of an NTFSInfo CSV.
//!
//! The file is read twice: a first pass indexes the directory tree (and the
//! extension records), a second one emits one row per file name with its full
//! path. The output field names match the NTFSInfo plugin where they overlap.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use super::windows_parsers::ntfs_attribute_values;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use ntfs_core::{
    AttributeBody, AttributeListEntry, FileName, FileReference, MftRecordHeader,
    StandardInformation, apply_fixup, parse_attributes,
};
use pyo3::prelude::*;

use crate::{
    Metadata, Output, Record, RunConfiguration, RunReport, Value,
    configuration::PluginConfiguration, errors::Error, parser::runner,
};

const SECTOR_SIZE: usize = 512;
const DEFAULT_RECORD_SIZE: usize = 1024;
const MAX_RECORD_SIZE: usize = 65536;
const ROOT_RECORD: u64 = 5;
const ORPHAN_ROOT: &str = "\\$OrphanFiles";
const MAX_PATH_DEPTH: usize = 256;
const READ_BUFFER_SIZE: usize = 1 << 20;

const RECORD_IN_USE: u16 = 0x01;
const RECORD_IS_DIRECTORY: u16 = 0x02;

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
#[cfg(test)]
const ATTR_END: u32 = 0xFFFF_FFFF;

const NAMESPACE_POSIX: u8 = 0;
const NAMESPACE_WIN32: u8 = 1;
const NAMESPACE_DOS: u8 = 2;
const NAMESPACE_WIN32_DOS: u8 = 3;

type Date = Option<DateTime<Utc>>;

#[pyfunction]
pub fn parse_mft(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> RunReport {
    match parse_mft_internal(input_file, run_config, plugin_config, metadata) {
        Ok(run_report) => run_report,
        Err(e) => {
            let mut run_report = RunReport::new();
            run_report.add_error(e.to_string());
            run_report
        }
    }
}

fn parse_mft_internal(
    input_file: &str,
    run_config: RunConfiguration,
    plugin_config: PluginConfiguration,
    metadata: Metadata,
) -> Result<RunReport, Error> {
    runner::validate_configuration(&run_config, &plugin_config)?;

    let mut file = File::open(Path::new(input_file))
        .map_err(|e| Error::FileRead(input_file.to_string(), e.to_string()))?;
    let input_size = file.metadata()?.len();
    let record_size = detect_record_size(&mut file)?;
    let mut reader = BufReader::with_capacity(READ_BUFFER_SIZE, file);

    let mut index = MftIndex::default();
    let mut declared_size = None;
    let mut report = for_each_record(&mut reader, record_size, true, |record| {
        if record.record_number == 0 {
            declared_size = record.data_size();
        }
        index.add(record);
        Ok(())
    })?;
    index.finish();
    if let Some(expected) = declared_size
        && input_size < expected
        && input_size.is_multiple_of(record_size as u64)
    {
        report.add_error(format!(
            "Truncated MFT: {input_size} bytes available, record 0 declares {expected}"
        ));
    }
    reader.seek(SeekFrom::Start(0))?;

    let parser_tree = plugin_config
        .get_data_type_mapping(None)?
        .field_mapping
        .as_ref()
        .map(|mapping| mapping.get_field_parser_tree())
        .unwrap_or_default();
    let output = Output::new(run_config, plugin_config, metadata, None)?;
    runner::with_output(output, &mut report, |output, report| {
        for_each_record(&mut reader, record_size, false, |record| {
            if record.base_record != 0 {
                return Ok(()); // merged into its base record through the index
            }
            for error in index.reference_errors(&record) {
                report.add_error(error);
            }
            for row in build_rows(&record, &mut index) {
                let mut mapped = Record::new();
                for (name, value) in row.0 {
                    parser_tree.set_field_value(&name, value, &mut mapped)?;
                }
                output.write(&mut mapped)?;
            }
            Ok(())
        })
        .map(|_| ())
    });
    Ok(report)
}

#[derive(Debug, Clone)]
struct FileNameAttr {
    attribute_id: u16,
    parent_record: u64,
    parent_sequence: u16,
    namespace: u8,
    name: String,
    created: Date,
    modified: Date,
    mft_modified: Date,
    accessed: Date,
    flags: u32,
}

#[derive(Debug, Clone)]
struct StandardInfo {
    created: Date,
    modified: Date,
    mft_modified: Date,
    accessed: Date,
    flags: u32,
    usn: Option<u64>,
    owner_id: Option<u32>,
    security_id: Option<u32>,
}

#[derive(Debug, Default, Clone)]
struct MftRecord {
    record_number: u64,
    sequence: u16,
    flags: u16,
    hard_links: u16,
    /// 0 for a base record.
    base_record: u64,
    standard_info: Option<StandardInfo>,
    file_names: Vec<FileNameAttr>,
    /// Data attribute fragments, including named streams; only VCN 0 carries size.
    data: Vec<DataStream>,
    attribute_list: AttributeList,
}

#[derive(Debug, Default, Clone)]
enum AttributeList {
    #[default]
    Absent,
    Resident(Vec<AttributeListEntry>),
    NonResident,
}

#[derive(Debug, Clone)]
struct DataStream {
    attribute_id: u16,
    start_vcn: u64,
    name: String,
    size: u64,
}

impl MftRecord {
    fn in_use(&self) -> bool {
        self.flags & RECORD_IN_USE != 0
    }

    fn is_directory(&self) -> bool {
        self.flags & RECORD_IS_DIRECTORY != 0
    }

    fn data_size(&self) -> Option<u64> {
        self.data
            .iter()
            .find(|stream| stream.name.is_empty() && stream.start_vcn == 0)
            .map(|stream| stream.size)
    }

    fn add_data(&mut self, stream: DataStream) {
        if !self
            .data
            .iter()
            .any(|existing| existing.name == stream.name && existing.start_vcn == stream.start_vcn)
        {
            self.data.push(stream);
        }
    }
}

fn sequence_matches(sequence: u16, in_use: bool, reference: u16) -> bool {
    sequence == reference || (!in_use && sequence == reference.wrapping_add(1))
}

fn le_u32(buf: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        buf.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn filetime_to_datetime(filetime: u64) -> Date {
    const UNIX_EPOCH_AS_FILETIME_SECONDS: i64 = 11_644_473_600;
    if filetime == 0 {
        return None;
    }
    let seconds = i64::try_from(filetime / 10_000_000).ok()? - UNIX_EPOCH_AS_FILETIME_SECONDS;
    let nanos = u32::try_from((filetime % 10_000_000) * 100).ok()?;
    DateTime::from_timestamp(seconds, nanos)
}

fn parse_file_name(content: &[u8]) -> Result<FileNameAttr, Error> {
    let name = FileName::parse(content)?;
    Ok(FileNameAttr {
        attribute_id: 0,
        parent_record: name.parent.record_number,
        parent_sequence: name.parent.sequence,
        namespace: name.namespace,
        name: name.name,
        created: filetime_to_datetime(name.created.0),
        modified: filetime_to_datetime(name.modified.0),
        mft_modified: filetime_to_datetime(name.mft_modified.0),
        accessed: filetime_to_datetime(name.accessed.0),
        flags: name.flags,
    })
}

fn parse_standard_info(content: &[u8]) -> Result<StandardInfo, Error> {
    let info = StandardInformation::parse(content)?;
    Ok(StandardInfo {
        created: filetime_to_datetime(info.created.0),
        modified: filetime_to_datetime(info.modified.0),
        mft_modified: filetime_to_datetime(info.mft_modified.0),
        accessed: filetime_to_datetime(info.accessed.0),
        flags: info.file_attributes,
        usn: info.usn,
        // ntfs-core does not yet expose the NTFS 3.x owner id.
        owner_id: if content.len() >= 0x48 {
            le_u32(content, 0x30)
        } else {
            None
        },
        security_id: info.security_id,
    })
}

/// Decode a complete, on-disk record. Do not interpret record slack as attributes.
fn parse_record(raw: &mut [u8], index: u64) -> Result<MftRecord, Error> {
    let header = MftRecordHeader::parse(raw)?;
    if header.allocated_size as usize != raw.len() {
        return Err(Error::Mft(
            "allocated size differs from the record slot".into(),
        ));
    }
    if header.is_corrupt() {
        return Err(Error::Mft("BAAD record".into()));
    }
    let sectors = usize::from(header.usa_count)
        .checked_sub(1)
        .filter(|count| *count > 0)
        .ok_or_else(|| Error::Mft("missing update sequence array".into()))?;
    // A standalone MFT lacks the boot sector. Infer the protection stride from
    // the complete record and its USA; accept only NTFS's 512/4096-byte units.
    let stride = raw.len() / sectors;
    if !raw.len().is_multiple_of(sectors) || !matches!(stride, 512 | 4096) {
        return Err(Error::Mft(
            "inconsistent update sequence array geometry".into(),
        ));
    }
    apply_fixup(raw, stride)?;
    let used = header.used_size as usize;
    let first = usize::from(header.first_attribute_offset);
    if used > raw.len() || first < 0x30 || first + 4 > used {
        return Err(Error::Mft(
            "invalid used size or first attribute offset".into(),
        ));
    }
    let buf = &raw[..used];
    let mut record = MftRecord {
        // Physical slot is authoritative, including pre-XP headers and stale
        // record-number fields. References keep all 48 bits of their record id.
        record_number: index,
        sequence: header.sequence_number,
        hard_links: header.hard_link_count,
        flags: header.flags,
        base_record: header.base_record,
        ..Default::default()
    };
    let attributes = parse_attributes(buf, first)?;
    let end = attributes
        .last()
        .map_or(first, |attr| attr.offset + attr.length as usize);
    if buf.get(end..end + 4) != Some(&0xffff_ffffu32.to_le_bytes()) {
        return Err(Error::Mft("missing attribute end marker".into()));
    }
    for attr in attributes {
        match attr.type_code {
            ATTR_STANDARD_INFORMATION => {
                let content = attr
                    .resident_content(buf)
                    .ok_or_else(|| Error::Mft("non-resident standard information".into()))?;
                record.standard_info = Some(parse_standard_info(content)?);
            }
            ATTR_FILE_NAME => {
                let content = attr
                    .resident_content(buf)
                    .ok_or_else(|| Error::Mft("non-resident filename".into()))?;
                let mut name = parse_file_name(content)?;
                name.attribute_id = attr.attribute_id;
                record.file_names.push(name);
            }
            ATTR_DATA => {
                let (start_vcn, size) = match attr.body {
                    AttributeBody::Resident { content_length, .. } => {
                        (0, u64::from(content_length))
                    }
                    AttributeBody::NonResident {
                        start_vcn,
                        real_size,
                        ..
                    } => (start_vcn, real_size),
                };
                record.add_data(DataStream {
                    attribute_id: attr.attribute_id,
                    start_vcn,
                    name: attr.name.unwrap_or_default(),
                    size,
                });
            }
            0x20 => {
                record.attribute_list = if let Some(content) = attr.resident_content(buf) {
                    AttributeList::Resident(ntfs_core::parse_attribute_list(content)?)
                } else {
                    AttributeList::NonResident
                };
            }
            // A full scan of the collected MFT recovers extension metadata via
            // the base reference, even when $ATTRIBUTE_LIST is non-resident.
            // Neither data run contents nor the volume are read here.
            _ => {}
        }
    }
    Ok(record)
}

/// Read the allocated record size from the first record header, leaving the
/// reader at the start of the file.
fn detect_record_size<R: Read + Seek>(reader: &mut R) -> Result<usize, Error> {
    let mut head = [0u8; 0x30];
    reader.seek(SeekFrom::Start(0))?;
    let read = read_up_to(reader, &mut head)?;
    let hint = le_u32(&head, 0x1c).unwrap_or(0) as usize;
    let plausible =
        |size: usize| (SECTOR_SIZE..=MAX_RECORD_SIZE).contains(&size) && size.is_power_of_two();
    // Record zero itself may be damaged, including a plausible but incorrect
    // allocated size. Confirm framing against a fully decoded record.
    let candidates = [
        hint,
        DEFAULT_RECORD_SIZE,
        512,
        2048,
        4096,
        8192,
        16384,
        32768,
        65536,
    ];
    let mut found = None;
    if read == head.len() {
        'sizes: for size in candidates.into_iter().filter(|size| plausible(*size)) {
            let mut buf = vec![0u8; size];
            for slot in 0..=16u64 {
                reader.seek(SeekFrom::Start(slot * size as u64))?;
                if read_up_to(reader, &mut buf)? < size {
                    break;
                }
                if parse_record(&mut buf, slot).is_ok() {
                    found = Some(size);
                    break 'sizes;
                }
            }
        }
    }
    let result = found.ok_or_else(|| {
        Error::Mft("cannot determine record size: no valid FILE record in the MFT prefix".into())
    });
    reader.seek(SeekFrom::Start(0))?;
    result
}

/// Fill `buf` as far as the reader allows; returns the number of bytes read.
fn read_up_to<R: Read>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Call `operation` for every decodable record of the file, in file order.
fn for_each_record<R: Read>(
    reader: &mut R,
    record_size: usize,
    report_errors: bool,
    mut operation: impl FnMut(MftRecord) -> Result<(), Error>,
) -> Result<RunReport, Error> {
    let mut report = RunReport::new();
    let mut buf = vec![0u8; record_size];
    let mut index = 0u64;
    loop {
        let read = read_up_to(reader, &mut buf)?;
        if read == 0 {
            return Ok(report);
        }
        if read < record_size {
            if report_errors {
                report.add_error(format!(
                    "Truncated MFT record {index}: {read}/{record_size} bytes"
                ));
            }
            return Ok(report);
        }
        // Never-used slots have no forensic identity. Other invalid records
        // are counted in the first pass, while valid later records survive.
        if buf.iter().all(|byte| *byte == 0) {
            if report_errors && matches!(index, 0 | ROOT_RECORD) {
                report.add_error(format!("Missing mandatory MFT record {index}"));
            }
        } else {
            match parse_record(&mut buf, index) {
                Ok(record) => operation(record)?,
                Err(error) if report_errors => {
                    report.add_error(format!("MFT record {index}: {error}"))
                }
                Err(_) => {}
            }
        }
        index += 1;
    }
}

#[derive(Debug)]
struct Node {
    parent: u64,
    parent_sequence: u16,
    name: String,
    sequence: u16,
    in_use: bool,
}

impl Node {
    /// A deleted directory has its sequence number incremented.
    fn is_valid_parent_of(&self, child_sequence: u16) -> bool {
        sequence_matches(self.sequence, self.in_use, child_sequence)
    }
}

/// Directory tree built in the first pass, used to resolve full paths.
#[derive(Default)]
struct MftIndex {
    nodes: Vec<Option<Node>>,
    path_cache: HashMap<u64, String>,
    pending_directories: Vec<MftRecord>,
    /// Extension records keyed by the complete base file reference.
    extensions: HashMap<(u64, u16), Vec<MftRecord>>,
}

fn name_rank(namespace: u8) -> u8 {
    match namespace {
        NAMESPACE_WIN32 | NAMESPACE_WIN32_DOS => 0,
        NAMESPACE_POSIX => 1,
        NAMESPACE_DOS => 2,
        _ => 3,
    }
}

/// Pick the long (Win32/POSIX) name, falling back to the DOS name.
fn preferred_name(names: &[FileNameAttr]) -> Option<&FileNameAttr> {
    names.iter().min_by_key(|item| name_rank(item.namespace))
}

impl MftIndex {
    fn add(&mut self, record: MftRecord) {
        if record.base_record != 0 {
            self.add_extension(record);
        } else if record.is_directory() {
            self.pending_directories.push(record);
        }
    }

    fn finish(&mut self) {
        for record in std::mem::take(&mut self.pending_directories) {
            let record = self.complete_record(&record);
            let Some(name) = preferred_name(&record.file_names) else {
                continue;
            };
            let Ok(slot) = usize::try_from(record.record_number) else {
                continue;
            };
            let node = Node {
                parent: name.parent_record,
                parent_sequence: name.parent_sequence,
                name: name.name.clone(),
                sequence: record.sequence,
                in_use: record.in_use(),
            };
            if self.nodes.len() <= slot {
                self.nodes.resize_with(slot + 1, || None);
            }
            self.nodes[slot] = Some(node);
        }
    }

    fn add_extension(&mut self, record: MftRecord) {
        let reference = FileReference::from_u64(record.base_record);
        self.extensions
            .entry((reference.record_number, reference.sequence))
            .or_default()
            .push(record);
    }

    fn extensions_for(&self, base: &MftRecord) -> Option<&Vec<MftRecord>> {
        self.extensions
            .get(&(base.record_number, base.sequence))
            .or_else(|| {
                if !base.in_use() {
                    self.extensions
                        .get(&(base.record_number, base.sequence.wrapping_sub(1)))
                } else {
                    None
                }
            })
    }

    fn complete_record(&self, base: &MftRecord) -> MftRecord {
        let mut record = base.clone();
        for extension in self.extensions_for(base).into_iter().flatten() {
            // A live file's current extension cannot be a deallocated remnant.
            // An absent list is distinct from an unavailable nonresident list.
            if base.in_use() != extension.in_use()
                || (base.in_use() && matches!(base.attribute_list, AttributeList::Absent))
            {
                continue;
            }
            let listed = |type_code: u32, id: u16, name: Option<&str>, vcn: u64| match &base
                .attribute_list
            {
                AttributeList::Resident(list) => list.iter().any(|entry| {
                    entry.type_code == type_code
                        && entry.attribute_id == id
                        && entry.name.as_deref() == name
                        && entry.start_vcn == vcn
                        && entry.base_reference.record_number == extension.record_number
                        && sequence_matches(
                            extension.sequence,
                            extension.in_use(),
                            entry.base_reference.sequence,
                        )
                }),
                _ => true,
            };
            record.file_names.extend(
                extension
                    .file_names
                    .iter()
                    .filter(|name| listed(ATTR_FILE_NAME, name.attribute_id, None, 0))
                    .cloned(),
            );
            for stream in &extension.data {
                let name = if stream.name.is_empty() {
                    None
                } else {
                    Some(stream.name.as_str())
                };
                if listed(ATTR_DATA, stream.attribute_id, name, stream.start_vcn) {
                    record.add_data(stream.clone());
                }
            }
        }
        record
    }

    fn reference_errors(&self, base: &MftRecord) -> Vec<String> {
        let mut errors = Vec::new();
        if let AttributeList::Resident(list) = &base.attribute_list {
            for entry in list {
                let reference = entry.base_reference;
                let source = if reference.record_number == base.record_number
                    && sequence_matches(base.sequence, base.in_use(), reference.sequence)
                {
                    Some(base)
                } else {
                    self.extensions_for(base)
                        .into_iter()
                        .flatten()
                        .find(|extension| {
                            extension.record_number == reference.record_number
                                && base.in_use() == extension.in_use()
                                && sequence_matches(
                                    extension.sequence,
                                    extension.in_use(),
                                    reference.sequence,
                                )
                        })
                };
                let present = source.is_some_and(|source| match entry.type_code {
                    ATTR_DATA => source.data.iter().any(|data| {
                        data.attribute_id == entry.attribute_id
                            && data.start_vcn == entry.start_vcn
                            && data.name == entry.name.as_deref().unwrap_or("")
                    }),
                    ATTR_FILE_NAME => source
                        .file_names
                        .iter()
                        .any(|name| name.attribute_id == entry.attribute_id),
                    _ => true,
                });
                if !present {
                    errors.push(format!("MFT record {}: missing/stale listed attribute {:#x}/{} in record {} sequence {}",
                        base.record_number, entry.type_code, entry.attribute_id, reference.record_number, reference.sequence));
                }
            }
        }
        let complete = self.complete_record(base);
        if complete
            .data_size()
            .is_some_and(|size| i64::try_from(size).is_err())
        {
            errors.push(format!(
                "MFT record {}: file_size exceeds the supported signed 64-bit output range",
                base.record_number
            ));
        }
        if base
            .standard_info
            .as_ref()
            .and_then(|info| info.usn)
            .is_some_and(|usn| i64::try_from(usn).is_err())
        {
            errors.push(format!(
                "MFT record {}: usn_number exceeds the supported signed 64-bit output range",
                base.record_number
            ));
        }
        for fragment in complete
            .data
            .iter()
            .filter(|fragment| fragment.start_vcn != 0)
        {
            if !complete
                .data
                .iter()
                .any(|data| data.name == fragment.name && data.start_vcn == 0)
            {
                errors.push(format!(
                    "MFT record {}: missing VCN-0 metadata for data stream {:?}",
                    base.record_number, fragment.name
                ));
            }
        }
        errors
    }

    fn node(&self, record: u64) -> Option<&Node> {
        self.nodes.get(usize::try_from(record).ok()?)?.as_ref()
    }

    /// Full path of a directory ("\" for the root); orphans live under
    /// `\$OrphanFiles`.
    fn directory_path(&mut self, record: u64, sequence: u16) -> String {
        match self.node(record) {
            Some(node) if node.is_valid_parent_of(sequence) => {}
            _ => return ORPHAN_ROOT.to_string(),
        }
        if record == ROOT_RECORD {
            return "\\".to_string();
        }
        if let Some(cached) = self.path_cache.get(&record) {
            return cached.clone();
        }
        let path = self.compute_path(record);
        self.path_cache.insert(record, path.clone());
        path
    }

    /// `record` must have been validated as an existing node.
    fn compute_path(&self, record: u64) -> String {
        let mut parts: Vec<&str> = Vec::new();
        let mut seen: Vec<u64> = Vec::new();
        let mut base = ORPHAN_ROOT.to_string();
        let mut current = record;
        let Some(mut node) = self.node(record) else {
            return base;
        };
        loop {
            if seen.contains(&current) || parts.len() > MAX_PATH_DEPTH {
                break; // loop in a corrupted tree
            }
            seen.push(current);
            parts.push(&node.name);
            let Some(parent) = self.node(node.parent) else {
                break;
            };
            if !parent.is_valid_parent_of(node.parent_sequence) {
                break;
            }
            if node.parent == ROOT_RECORD {
                base.clear();
                break;
            }
            if let Some(cached) = self.path_cache.get(&node.parent) {
                base = cached.trim_end_matches('\\').to_string();
                break;
            }
            current = node.parent;
            node = parent;
        }
        parts.reverse();
        format!("{base}\\{}", parts.join("\\"))
    }
}

fn int(value: u64) -> Value {
    i64::try_from(value)
        .map(Value::Int)
        .unwrap_or(Value::Null())
}

fn add_date(row: &mut Record, name: &str, date: Date) {
    if let Some(date) = date {
        row.add(name, Value::Date(date));
    }
}

/// Extension of a file name, with its dot; leading dots do not count.
fn extension_of(name: &str) -> &str {
    let trimmed = name.trim_start_matches('.');
    let skipped = name.len() - trimmed.len();
    match trimmed.rfind('.') {
        Some(position) => &name[skipped + position..],
        None => "",
    }
}

/// One row per non-DOS file name of a base record (the DOS alias becomes
/// `short_name`); a record with only a DOS name still produces a row.
fn build_rows(record: &MftRecord, index: &mut MftIndex) -> Vec<Record> {
    let merged = index.complete_record(record);
    let record = &merged;
    let file_names = &record.file_names;
    if file_names.is_empty() {
        return Vec::new();
    }
    let data_size = record.data_size();
    let mut streams = IndexMap::new();
    for stream in record.data.iter().filter(|stream| !stream.name.is_empty()) {
        let size = streams.entry(&stream.name).or_insert(None);
        if stream.start_vcn == 0 {
            *size = Some(stream.size);
        }
    }
    let ads = streams
        .into_iter()
        .map(|(name, size)| {
            let mut stream = Record::new();
            stream.add("name", Value::String(name.clone()));
            stream.add("size", size.map(Value::UInt).unwrap_or(Value::Null()));
            Value::Object(stream)
        })
        .collect::<Vec<_>>();

    let long_names: Vec<&FileNameAttr> = file_names
        .iter()
        .filter(|item| item.namespace != NAMESPACE_DOS)
        .collect();
    let selected: Vec<&FileNameAttr> = if long_names.is_empty() {
        file_names.iter().collect()
    } else {
        long_names
    };

    let info = record.standard_info.as_ref();
    let mut rows = Vec::new();
    let mut seen: Vec<(u64, u16, &str)> = Vec::new();
    for file_name in selected {
        let key = (
            file_name.parent_record,
            file_name.parent_sequence,
            file_name.name.as_str(),
        );
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);

        let parent = index.directory_path(file_name.parent_record, file_name.parent_sequence);
        let file_path = if record.record_number == ROOT_RECORD {
            "\\".to_string()
        } else if parent.ends_with('\\') {
            format!("{parent}{}", file_name.name)
        } else {
            format!("{parent}\\{}", file_name.name)
        };
        let extension = if record.is_directory() {
            ""
        } else {
            extension_of(&file_name.name)
        };

        let mut row = Record::new();
        row.add("file_path", Value::String(file_path));
        row.add("parent", Value::String(parent));
        row.add("file_name", Value::String(file_name.name.clone()));
        row.add("extension", Value::String(extension.to_string()));
        if let Some(size) = data_size {
            row.add("file_size", int(size));
        }
        row.add("file_in_use", Value::Bool(record.in_use()));
        row.add("sequence_number", int(u64::from(record.sequence)));
        row.add("record_number", int(record.record_number));
        row.add(
            "parent_sequence_number",
            int(u64::from(file_name.parent_sequence)),
        );
        row.add("parent_record_number", int(file_name.parent_record));
        row.add("hard_link_count", int(u64::from(record.hard_links)));
        let same_parent = |name: &&FileNameAttr| {
            name.parent_record == file_name.parent_record
                && name.parent_sequence == file_name.parent_sequence
        };
        let aliases: Vec<_> = file_names
            .iter()
            .filter(same_parent)
            .filter(|name| name.namespace == NAMESPACE_DOS)
            .collect();
        let long_count = file_names
            .iter()
            .filter(same_parent)
            .filter(|name| name.namespace != NAMESPACE_DOS)
            .count();
        if aliases.len() == 1 && long_count <= 1 {
            row.add("short_name", Value::String(aliases[0].name.clone()));
        }
        row.add("ads", Value::Array(ads.clone()));

        let flags = info.map_or(file_name.flags, |info| info.flags);
        row.extend(ntfs_attribute_values(flags));
        row.add(
            "file_attribute_raw",
            Value::String(format!("0x{flags:08X}")),
        );
        row.add(
            "file_attributes_directory",
            Value::Bool(record.is_directory()),
        );

        if let Some(info) = info {
            if let Some(usn) = info.usn {
                row.add("usn_number", int(usn));
            }
            if let Some(owner_id) = info.owner_id {
                row.add("owner_id", int(u64::from(owner_id)));
            }
            if let Some(security_id) = info.security_id {
                row.add("security_id", int(u64::from(security_id)));
            }
            add_date(&mut row, "si_creation_date", info.created);
            add_date(&mut row, "si_lastmod_date", info.modified);
            add_date(&mut row, "si_lastaccess_date", info.accessed);
            add_date(&mut row, "si_lastchange_date", info.mft_modified);
        }
        row.add("file_name_flags", int(u64::from(file_name.namespace)));
        add_date(&mut row, "fn_creation_date", file_name.created);
        add_date(&mut row, "fn_lastmod_date", file_name.modified);
        add_date(&mut row, "fn_lastaccess_date", file_name.accessed);
        add_date(&mut row, "fn_lastchange_date", file_name.mft_modified);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const RECORD_SIZE: usize = 1024;
    const T0: u64 = 1_600_000_000;

    fn filetime(unix_seconds: u64) -> u64 {
        116_444_736_000_000_000 + unix_seconds * 10_000_000
    }

    fn attr(attr_type: u32, content: &[u8], name: &str) -> Vec<u8> {
        let raw_name: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let name_off = 24usize;
        let content_off = (name_off + raw_name.len() + 7) & !7;
        let length = (content_off + content.len() + 7) & !7;
        let mut out = vec![0u8; length];
        out[0..4].copy_from_slice(&attr_type.to_le_bytes());
        out[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        out[9] = name.encode_utf16().count() as u8;
        out[10..12].copy_from_slice(&(name_off as u16).to_le_bytes());
        out[0x10..0x14].copy_from_slice(&(content.len() as u32).to_le_bytes());
        out[0x14..0x16].copy_from_slice(&(content_off as u16).to_le_bytes());
        out[name_off..name_off + raw_name.len()].copy_from_slice(&raw_name);
        out[content_off..content_off + content.len()].copy_from_slice(content);
        out
    }

    fn standard_info(created: u64, modified: u64, flags: u32, usn: u64) -> Vec<u8> {
        let mut content = vec![0u8; 72];
        content[0..8].copy_from_slice(&created.to_le_bytes());
        for offset in [8, 16, 24] {
            content[offset..offset + 8].copy_from_slice(&modified.to_le_bytes());
        }
        content[32..36].copy_from_slice(&flags.to_le_bytes());
        content[48..52].copy_from_slice(&0x100u32.to_le_bytes());
        content[52..56].copy_from_slice(&0x200u32.to_le_bytes());
        content[64..72].copy_from_slice(&usn.to_le_bytes());
        attr(ATTR_STANDARD_INFORMATION, &content, "")
    }

    fn file_name(parent: u64, parent_seq: u64, name: &str, namespace: u8, created: u64) -> Vec<u8> {
        let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut content = vec![0u8; 66];
        content[0..8].copy_from_slice(&(parent | (parent_seq << 48)).to_le_bytes());
        for offset in [8, 16, 24, 32] {
            content[offset..offset + 8].copy_from_slice(&created.to_le_bytes());
        }
        content[56..60].copy_from_slice(&0x20u32.to_le_bytes());
        content[64] = name.encode_utf16().count() as u8;
        content[65] = namespace;
        content.extend(units);
        attr(ATTR_FILE_NAME, &content, "")
    }

    fn listed_data(record: u64, sequence: u16) -> Vec<u8> {
        let mut entry = vec![0u8; 32];
        entry[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        entry[4..6].copy_from_slice(&32u16.to_le_bytes());
        entry[16..24].copy_from_slice(&(record | (u64::from(sequence) << 48)).to_le_bytes());
        attr(0x20, &entry, "")
    }

    fn data(size: usize, name: &str) -> Vec<u8> {
        attr(ATTR_DATA, &vec![b'x'; size], name)
    }

    fn data_non_resident(real_size: u64) -> Vec<u8> {
        let mut out = vec![0u8; 0x48];
        out[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        out[4..8].copy_from_slice(&0x48u32.to_le_bytes());
        out[8] = 1;
        out[0x20..0x22].copy_from_slice(&0x40u16.to_le_bytes());
        out[0x28..0x30].copy_from_slice(&4096u64.to_le_bytes());
        out[0x30..0x38].copy_from_slice(&real_size.to_le_bytes());
        out[0x38..0x40].copy_from_slice(&real_size.to_le_bytes());
        out
    }

    fn record(index: u32, attrs: &[Vec<u8>], seq: u16, flags: u16, base: u64) -> Vec<u8> {
        let first = 0x38usize;
        let mut body: Vec<u8> = attrs.concat();
        body.extend(ATTR_END.to_le_bytes());
        let mut buf = vec![0u8; RECORD_SIZE];
        buf[0..4].copy_from_slice(b"FILE");
        buf[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        buf[6..8].copy_from_slice(&3u16.to_le_bytes());
        buf[0x10..0x12].copy_from_slice(&seq.to_le_bytes());
        buf[0x12..0x14].copy_from_slice(&1u16.to_le_bytes());
        buf[0x14..0x16].copy_from_slice(&(first as u16).to_le_bytes());
        buf[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        buf[0x18..0x1C].copy_from_slice(&((first + body.len()) as u32).to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&(RECORD_SIZE as u32).to_le_bytes());
        buf[0x20..0x28].copy_from_slice(&base.to_le_bytes());
        buf[0x2C..0x30].copy_from_slice(&index.to_le_bytes());
        buf[first..first + body.len()].copy_from_slice(&body);
        // Update sequence array: usn 1, protecting the last 2 bytes of each sector.
        buf[0x30..0x32].copy_from_slice(&1u16.to_le_bytes());
        for sector in 1..3usize {
            let end = sector * SECTOR_SIZE;
            let saved = [buf[end - 2], buf[end - 1]];
            buf[0x30 + sector * 2..0x32 + sector * 2].copy_from_slice(&saved);
            buf[end - 2..end].copy_from_slice(&1u16.to_le_bytes());
        }
        buf
    }

    fn build_mft() -> Vec<u8> {
        let si = |flags, usn| standard_info(filetime(T0), filetime(T0), flags, usn);
        let records = vec![
            // 0: $MFT itself
            record(
                0,
                &[
                    si(0x20, 0),
                    file_name(5, 5, "$MFT", 1, 0),
                    data_non_resident(4096),
                ],
                1,
                1,
                0,
            ),
            vec![0u8; RECORD_SIZE],
            vec![0u8; RECORD_SIZE],
            vec![0u8; RECORD_SIZE],
            vec![0u8; RECORD_SIZE],
            // 5: root
            record(5, &[si(0x6, 0), file_name(5, 5, ".", 1, 0)], 5, 3, 0),
            // 6: \Windows
            record(6, &[si(0x20, 0), file_name(5, 5, "Windows", 1, 0)], 1, 3, 0),
            // 7: \Windows\evil.exe with a short name and an ADS
            record(
                7,
                &[
                    standard_info(filetime(T0), filetime(T0 + 60), 0x22, 1234),
                    file_name(6, 1, "EVIL~1.EXE", NAMESPACE_DOS, 0),
                    file_name(6, 1, "evil.exe", NAMESPACE_WIN32, filetime(T0 + 5)),
                    data(300, ""),
                    data(5, "Zone.Identifier"),
                ],
                3,
                1,
                0,
            ),
            // 8: deleted file in \Windows
            record(
                8,
                &[si(0x20, 0), file_name(6, 1, "gone.txt", 1, 0), data(10, "")],
                1,
                0,
                0,
            ),
            // 9: deleted file whose parent (record 20) does not exist
            record(
                9,
                &[si(0x20, 0), file_name(20, 1, "lost.bin", 1, 0), data(0, "")],
                1,
                0,
                0,
            ),
            // 10: non-resident $DATA
            record(
                10,
                &[
                    si(0x20, 0),
                    file_name(5, 5, "big.dat", 1, 0),
                    data_non_resident(1 << 33),
                ],
                1,
                1,
                0,
            ),
            // 11: extension record holding the $DATA of record 12
            record(11, &[data(777, "")], 1, 1, (1 << 48) | 12),
            // 12: base record whose data lives in the extension record
            record(
                12,
                &[
                    si(0x20, 0),
                    file_name(5, 5, "split.dat", 1, 0),
                    listed_data(11, 1),
                ],
                1,
                1,
                0,
            ),
        ];
        records.concat()
    }

    fn index_and_records() -> (MftIndex, Vec<MftRecord>) {
        let mut records = Vec::new();
        for_each_record(&mut Cursor::new(build_mft()), RECORD_SIZE, true, |record| {
            records.push(record);
            Ok(())
        })
        .unwrap();
        let mut index = MftIndex::default();
        for record in &records {
            index.add(record.clone());
        }
        index.finish();
        (index, records)
    }

    fn rows_of(number: u64) -> Vec<Record> {
        let (mut index, records) = index_and_records();
        let record = records.iter().find(|r| r.record_number == number).unwrap();
        build_rows(record, &mut index)
    }

    fn get<'a>(row: &'a Record, name: &str) -> &'a Value {
        row.get(name)
            .unwrap_or_else(|| panic!("missing field {name}"))
    }

    fn ads_json(row: &Record) -> serde_json::Value {
        let mut json = String::new();
        Value::json_serialise_value(
            get(row, "ads"),
            &mut json,
            &crate::DateOutputCodec::Iso(),
            true,
        )
        .unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn ads_preserve_stream_names_and_zero_sizes() {
        let mut raw = record(
            7,
            &[
                file_name(5, 5, "streams.txt", 1, 0),
                data(3, ""),
                data(5, "Zone.Identifier"),
                data(0, "custom;\"stream"),
            ],
            1,
            1,
            0,
        );
        let record = parse_record(&mut raw, 7).unwrap();
        let rows = build_rows(&record, &mut MftIndex::default());
        assert_eq!(
            ads_json(&rows[0]),
            serde_json::json!([
                {"name": "Zone.Identifier", "size": 5},
                {"name": "custom;\"stream", "size": 0},
            ])
        );
    }

    #[test]
    fn ads_are_empty_arrays_without_named_streams() {
        assert_eq!(ads_json(&rows_of(6)[0]), serde_json::json!([]));
    }

    #[test]
    fn ads_preserve_unknown_sizes_and_full_width_sizes_once_per_stream() {
        let (mut index, records) = index_and_records();
        let mut record = records
            .iter()
            .find(|r| r.record_number == 7)
            .unwrap()
            .clone();
        record.data = vec![
            DataStream {
                attribute_id: 0,
                start_vcn: 2,
                name: "partial".into(),
                size: 999,
            },
            DataStream {
                attribute_id: 1,
                start_vcn: 4,
                name: "complete".into(),
                size: 777,
            },
            DataStream {
                attribute_id: 2,
                start_vcn: 0,
                name: "complete".into(),
                size: u64::MAX,
            },
        ];
        assert_eq!(
            ads_json(&build_rows(&record, &mut index)[0]),
            serde_json::json!([
                {"name": "partial", "size": null},
                {"name": "complete", "size": u64::MAX},
            ])
        );
        assert_eq!(index.reference_errors(&record).len(), 1);
    }

    #[test]
    fn detects_record_size_and_rejects_other_files() {
        assert_eq!(
            detect_record_size(&mut Cursor::new(build_mft())).unwrap(),
            1024
        );
        assert!(detect_record_size(&mut Cursor::new(vec![0u8; 2048])).is_err());
    }

    #[test]
    fn skips_zeroed_and_torn_records() {
        let (_, records) = index_and_records();
        assert!(records.iter().all(|r| r.record_number != 1));

        let mut torn = record(1, &[file_name(5, 5, "x", 1, 0)], 1, 1, 0);
        torn[510] ^= 0xFF;
        assert!(parse_record(&mut torn, 1).is_err());
    }

    #[test]
    fn decodes_attributes() {
        let (_, records) = index_and_records();
        let evil = records.iter().find(|r| r.record_number == 7).unwrap();
        assert_eq!(evil.sequence, 3);
        assert_eq!(evil.data_size(), Some(300));
        assert_eq!(
            evil.data
                .iter()
                .filter(|s| !s.name.is_empty())
                .map(|s| (s.name.as_str(), s.size))
                .collect::<Vec<_>>(),
            vec![("Zone.Identifier", 5)]
        );
        assert_eq!(evil.standard_info.as_ref().unwrap().usn, Some(1234));
        assert_eq!(evil.file_names.len(), 2);

        let big = records.iter().find(|r| r.record_number == 10).unwrap();
        assert_eq!(big.data_size(), Some(1 << 33));
    }

    #[test]
    fn resolves_paths_and_orphans() {
        let (mut index, _) = index_and_records();
        assert_eq!(index.directory_path(5, 5), "\\");
        assert_eq!(index.directory_path(6, 1), "\\Windows");
        // The directory was reused since: treat the child as an orphan.
        assert_eq!(index.directory_path(6, 9), ORPHAN_ROOT);
        assert_eq!(index.directory_path(20, 1), ORPHAN_ROOT);
    }

    #[test]
    fn builds_a_full_row() {
        let rows = rows_of(7);
        assert_eq!(rows.len(), 1, "the DOS alias must not get its own row");
        let row = &rows[0];
        assert_eq!(
            get(row, "file_path"),
            &Value::String("\\Windows\\evil.exe".into())
        );
        assert_eq!(get(row, "parent"), &Value::String("\\Windows".into()));
        assert_eq!(get(row, "extension"), &Value::String(".exe".into()));
        assert_eq!(get(row, "file_size"), &Value::Int(300));
        assert_eq!(get(row, "short_name"), &Value::String("EVIL~1.EXE".into()));
        assert_eq!(
            ads_json(row),
            serde_json::json!([{"name": "Zone.Identifier", "size": 5}])
        );
        assert_eq!(get(row, "usn_number"), &Value::Int(1234));
        assert_eq!(get(row, "file_attributes_hidden"), &Value::Bool(true));
        assert_eq!(get(row, "file_attributes_directory"), &Value::Bool(false));
        assert_eq!(get(row, "file_in_use"), &Value::Bool(true));
        assert_eq!(get(row, "sequence_number"), &Value::Int(3));
        assert_eq!(get(row, "file_name_flags"), &Value::Int(1));
        assert_eq!(
            get(row, "si_lastmod_date"),
            &Value::Date(DateTime::from_timestamp((T0 + 60) as i64, 0).unwrap())
        );
    }

    #[test]
    fn directories_and_root() {
        let rows = rows_of(6);
        assert_eq!(
            get(&rows[0], "file_path"),
            &Value::String("\\Windows".into())
        );
        assert_eq!(
            get(&rows[0], "file_attributes_directory"),
            &Value::Bool(true)
        );
        assert_eq!(get(&rows[0], "extension"), &Value::String(String::new()));
        assert_eq!(
            get(&rows_of(5)[0], "file_path"),
            &Value::String("\\".into())
        );
    }

    #[test]
    fn deleted_and_orphan_records() {
        let deleted = rows_of(8);
        assert_eq!(get(&deleted[0], "file_in_use"), &Value::Bool(false));
        assert_eq!(
            get(&deleted[0], "file_path"),
            &Value::String("\\Windows\\gone.txt".into())
        );
        let orphan = rows_of(9);
        assert_eq!(
            get(&orphan[0], "file_path"),
            &Value::String("\\$OrphanFiles\\lost.bin".into())
        );
    }

    #[test]
    fn size_comes_from_the_extension_record() {
        assert_eq!(get(&rows_of(12)[0], "file_size"), &Value::Int(777));
        assert_eq!(get(&rows_of(10)[0], "file_size"), &Value::Int(1 << 33));
    }

    #[test]
    fn extension_ignores_leading_dots() {
        assert_eq!(extension_of("a.tar.gz"), ".gz");
        assert_eq!(extension_of(".bashrc"), "");
        assert_eq!(extension_of("noext"), "");
        assert_eq!(extension_of("a."), ".");
    }
    #[test]
    fn full_width_references_match_ntfs_info() {
        use crate::parser::windows_parsers::win_frn_hex_parser;
        let (mut index, _) = index_and_records();
        let entry = MftRecord {
            record_number: (1 << 47) | 23,
            sequence: 0xffff,
            flags: RECORD_IN_USE,
            file_names: vec![parse_file_name(&file_name(6, 1, "wide.txt", 1, 0)[24..]).unwrap()],
            ..Default::default()
        };
        let row = build_rows(&entry, &mut index).remove(0);
        let frn = (u64::from(entry.sequence) << 48) | entry.record_number;
        for (name, value) in win_frn_hex_parser("")
            .0
            .parse(&format!("0x{frn:016X}"), "")
            .unwrap()
        {
            assert_eq!(row.get(&name), Some(&value));
        }
    }

    #[test]
    fn hard_links_keep_their_own_parent_dates_and_dos_alias() {
        let mut raw = record(
            15,
            &[
                file_name(6, 1, "first.txt", 1, filetime(T0)),
                file_name(6, 1, "FIRST~1.TXT", 2, 0),
                file_name(5, 5, "other.txt", 1, filetime(T0 + 10)),
                file_name(5, 5, "OTHER~1.TXT", 2, 0),
            ],
            1,
            1,
            0,
        );
        let entry = parse_record(&mut raw, 15).unwrap();
        let (mut index, _) = index_and_records();
        let rows = build_rows(&entry, &mut index);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            get(&rows[0], "file_path"),
            &Value::String("\\Windows\\first.txt".into())
        );
        assert_eq!(
            get(&rows[1], "file_path"),
            &Value::String("\\other.txt".into())
        );
        assert_eq!(
            get(&rows[1], "short_name"),
            &Value::String("OTHER~1.TXT".into())
        );
        assert_ne!(
            get(&rows[0], "fn_creation_date"),
            get(&rows[1], "fn_creation_date")
        );
    }

    #[test]
    fn rejects_attributes_whose_content_overlaps_the_next_attribute() {
        let mut raw = record(1, &[file_name(5, 5, "x", 1, 0), data(20, "")], 1, 1, 0);
        // Content of $FN would spill into the following $DATA, inside the record.
        raw[0x48..0x4c].copy_from_slice(&300u32.to_le_bytes());
        assert!(parse_record(&mut raw, 1).is_err());
    }

    #[test]
    fn rejects_extensions_from_a_previous_allocation() {
        let mut index = MftIndex::default();
        let mut base = record(12, &[file_name(5, 5, "current.txt", 1, 0)], 2, 1, 0);
        let base = parse_record(&mut base, 12).unwrap();
        let mut ext = record(11, &[data(777, "")], 1, 1, (1 << 48) | 12);
        index.add(parse_record(&mut ext, 11).unwrap());
        index.finish();
        let rows = build_rows(&base, &mut index);
        assert_eq!(rows[0].get("file_size"), None);
    }

    #[test]
    fn zero_timestamps_stay_absent_and_pre_epoch_precision_is_preserved() {
        assert_eq!(filetime_to_datetime(0), None);
        let date = filetime_to_datetime(116_444_736_000_000_000 - 1).unwrap();
        assert_eq!(date.timestamp(), -1);
        assert_eq!(date.timestamp_subsec_nanos(), 999_999_900);
    }

    #[test]
    fn resident_attribute_list_rejects_stale_or_unlisted_extensions() {
        for (listed_record, listed_sequence) in [(11u64, 2u64), (13, 1)] {
            let mut list = vec![0u8; 32];
            list[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
            list[4..6].copy_from_slice(&32u16.to_le_bytes());
            list[16..24].copy_from_slice(&(listed_record | (listed_sequence << 48)).to_le_bytes());
            let mut base = record(
                12,
                &[file_name(5, 5, "current.txt", 1, 0), attr(0x20, &list, "")],
                2,
                1,
                0,
            );
            let base = parse_record(&mut base, 12).unwrap();
            let mut ext = record(11, &[data(777, "")], 1, 1, (2 << 48) | 12);
            let mut index = MftIndex::default();
            index.add(parse_record(&mut ext, 11).unwrap());
            index.finish();
            assert_eq!(build_rows(&base, &mut index)[0].get("file_size"), None);
        }
    }

    #[test]
    fn allocated_record_size_must_match_the_slot() {
        let mut raw = record(1, &[file_name(5, 5, "x", 1, 0)], 1, 1, 0);
        raw[0x1c..0x20].copy_from_slice(&4096u32.to_le_bytes());
        assert!(parse_record(&mut raw, 1).is_err());
    }

    #[test]
    fn reads_upstream_raw_mft_without_a_volume() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(&include_bytes!("../../test_data/mft/tiny.mft.gz")[..])
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes.len(), 262144);
        let mut records = Vec::new();
        let report = for_each_record(&mut Cursor::new(&bytes), RECORD_SIZE, true, |record| {
            records.push(record);
            Ok(())
        })
        .unwrap();
        assert_eq!(report.num_errors, 0);
        let file8 = records
            .iter()
            .find(|record| record.record_number == 32)
            .unwrap();
        let mut index = MftIndex::default();
        for entry in &records {
            index.add(entry.clone());
        }
        index.finish();
        let rows = build_rows(file8, &mut index);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            get(&rows[0], "file_path"),
            &Value::String("\\file8.txt".into())
        );
        assert_eq!(get(&rows[0], "file_size"), &Value::Int(357));
        assert_eq!(get(&rows[0], "sequence_number"), &Value::Int(3));
        assert_eq!(get(&rows[0], "security_id"), &Value::Int(260));
        assert_eq!(get(&rows[0], "owner_id"), &Value::Int(0));
    }
    #[test]
    fn reads_4096_byte_records_with_512_or_4096_byte_protection_units() {
        for sector_size in [512, 4096] {
            let small = record(0, &[file_name(5, 5, "large-record.txt", 1, 0)], 1, 1, 0);
            let used = le_u32(&small, 0x18).unwrap() as usize;
            let count = 4096 / sector_size + 1;
            let first = (0x30 + count * 2 + 7) & !7;
            let mut large = vec![0u8; 4096];
            large[..0x30].copy_from_slice(&small[..0x30]);
            large[6..8].copy_from_slice(&(count as u16).to_le_bytes());
            large[0x14..0x16].copy_from_slice(&(first as u16).to_le_bytes());
            large[0x18..0x1c].copy_from_slice(&((first + used - 0x38) as u32).to_le_bytes());
            large[0x1c..0x20].copy_from_slice(&4096u32.to_le_bytes());
            large[first..first + used - 0x38].copy_from_slice(&small[0x38..used]);
            large[0x30..0x32].copy_from_slice(&0xaaaau16.to_le_bytes());
            for i in 1..count {
                large[i * sector_size - 2..i * sector_size]
                    .copy_from_slice(&0xaaaau16.to_le_bytes());
            }
            let mut input = Cursor::new(large);
            assert_eq!(detect_record_size(&mut input).unwrap(), 4096);
            let mut records = Vec::new();
            let report = for_each_record(&mut input, 4096, true, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
            assert_eq!(report.num_errors, 0);
            assert_eq!(records[0].file_names[0].name, "large-record.txt");
        }
    }

    #[test]
    fn resident_attribute_list_accepts_the_matching_extension_attribute() {
        let mut list = vec![0u8; 32];
        list[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        list[4..6].copy_from_slice(&32u16.to_le_bytes());
        list[16..24].copy_from_slice(&((1u64 << 48) | 11).to_le_bytes());
        let mut base = record(
            12,
            &[file_name(5, 5, "listed.txt", 1, 0), attr(0x20, &list, "")],
            2,
            1,
            0,
        );
        let base = parse_record(&mut base, 12).unwrap();
        let mut ext = record(11, &[data(777, "")], 1, 1, (2 << 48) | 12);
        let mut index = MftIndex::default();
        index.add(parse_record(&mut ext, 11).unwrap());
        index.finish();
        assert_eq!(
            get(&build_rows(&base, &mut index)[0], "file_size"),
            &Value::Int(777)
        );
        let mut wrong_id = base.clone();
        let AttributeList::Resident(list) = &mut wrong_id.attribute_list else {
            panic!("resident list");
        };
        list[0].attribute_id = 9;
        assert_eq!(build_rows(&wrong_id, &mut index)[0].get("file_size"), None);
    }

    #[test]
    fn filename_references_keep_all_48_record_bits_and_16_sequence_bits() {
        let parent = (1u64 << 47) | 7;
        let mut raw = record(
            1,
            &[file_name(parent, 0xffff, "wide-parent.txt", 1, 0)],
            1,
            1,
            0,
        );
        let parsed = parse_record(&mut raw, 1).unwrap();
        assert_eq!(parsed.file_names[0].parent_record, parent);
        assert_eq!(parsed.file_names[0].parent_sequence, 0xffff);
    }

    #[test]
    fn deleted_directory_sequence_wrap_and_parent_cycles_are_bounded() {
        assert!(sequence_matches(0, false, 0xffff));
        assert!(!sequence_matches(0, true, 0xffff));
        let mut index = MftIndex::default();
        for (number, parent) in [(6u32, 7u64), (7, 6)] {
            let mut raw = record(number, &[file_name(parent, 1, "cycle", 1, 0)], 1, 3, 0);
            index.add(parse_record(&mut raw, u64::from(number)).unwrap());
        }
        index.finish();
        let path = index.directory_path(6, 1);
        assert!(path.starts_with(ORPHAN_ROOT));
        assert!(path.len() < 100);
        assert_eq!(index.directory_path(5, 5), ORPHAN_ROOT);
    }

    #[test]
    fn continuation_fragments_do_not_supply_stream_sizes() {
        let mut data = data_non_resident(999);
        data[0x10..0x18].copy_from_slice(&16u64.to_le_bytes());
        let mut raw = record(1, &[file_name(5, 5, "fragment.txt", 1, 0), data], 1, 1, 0);
        assert_eq!(parse_record(&mut raw, 1).unwrap().data_size(), None);
    }
    #[test]
    fn a_live_base_does_not_acquire_a_freed_extension_without_a_list() {
        let mut base = record(
            12,
            &[file_name(5, 5, "new.doc", 1, 0), data(0, "")],
            2,
            1,
            0,
        );
        let base = parse_record(&mut base, 12).unwrap();
        let mut ext = record(
            11,
            &[file_name(5, 5, "old.doc", 1, 0), data(4, "removed-ads")],
            3,
            0,
            (2 << 48) | 12,
        );
        let mut index = MftIndex::default();
        index.add(parse_record(&mut ext, 11).unwrap());
        index.finish();
        let rows = build_rows(&base, &mut index);
        assert_eq!(rows.len(), 1);
        assert_eq!(ads_json(&rows[0]), serde_json::json!([]));
        assert_eq!(get(&rows[0], "file_size"), &Value::Int(0));
    }

    #[test]
    fn ambiguous_dos_aliases_in_one_parent_are_not_attached_to_both_links() {
        let mut raw = record(
            15,
            &[
                file_name(6, 1, "first long.txt", 1, 0),
                file_name(6, 1, "FIRSTL~1.TXT", 2, 0),
                file_name(6, 1, "other long.txt", 1, 0),
                file_name(6, 1, "OTHERL~1.TXT", 2, 0),
            ],
            1,
            1,
            0,
        );
        let base = parse_record(&mut raw, 15).unwrap();
        let (mut index, _) = index_and_records();
        let rows = build_rows(&base, &mut index);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.get("short_name").is_none()));
    }

    #[test]
    fn missing_attribute_end_marker_is_an_error() {
        let mut raw = record(
            1,
            &[file_name(5, 5, "cut.txt", 1, 0), data(10, "")],
            1,
            1,
            0,
        );
        let end = 0x38 + le_u32(&raw, 0x38 + 4).unwrap();
        raw[0x18..0x1c].copy_from_slice(&end.to_le_bytes());
        assert!(parse_record(&mut raw, 1).is_err());
    }
    #[test]
    fn missing_listed_extension_or_vcn_zero_is_reported_without_a_false_zero_size() {
        let mut raw = record(
            12,
            &[file_name(5, 5, "partial.txt", 1, 0), listed_data(11, 1)],
            1,
            1,
            0,
        );
        let base = parse_record(&mut raw, 12).unwrap();
        let mut index = MftIndex::default();
        index.finish();
        assert_eq!(index.reference_errors(&base).len(), 1);
        assert_eq!(build_rows(&base, &mut index)[0].get("file_size"), None);
        let mut tail = data_non_resident(0);
        tail[0x10..0x18].copy_from_slice(&16u64.to_le_bytes());
        let mut raw = record(12, &[file_name(5, 5, "partial.txt", 1, 0), tail], 1, 1, 0);
        let base = parse_record(&mut raw, 12).unwrap();
        assert_eq!(index.reference_errors(&base).len(), 1);
        assert_eq!(build_rows(&base, &mut index)[0].get("file_size"), None);
        let mut raw = record(
            12,
            &[file_name(5, 5, "empty.txt", 1, 0), data(0, "")],
            1,
            1,
            0,
        );
        let base = parse_record(&mut raw, 12).unwrap();
        assert!(index.reference_errors(&base).is_empty());
        assert_eq!(
            get(&build_rows(&base, &mut index)[0], "file_size"),
            &Value::Int(0)
        );
    }
    #[test]
    fn out_of_range_sizes_are_reported_instead_of_silently_saturated() {
        let mut raw = record(
            12,
            &[
                file_name(5, 5, "oversized.txt", 1, 0),
                data_non_resident(u64::MAX),
            ],
            1,
            1,
            0,
        );
        let base = parse_record(&mut raw, 12).unwrap();
        let mut index = MftIndex::default();
        index.finish();
        let rows = build_rows(&base, &mut index);
        assert_eq!(rows[0].get("file_size"), Some(&Value::Null()));
        assert_eq!(index.reference_errors(&base).len(), 1);
    }
}
