use blake3::Hasher;
use chrono::{DateTime, Utc};

const HASH_PREFIX: &[u8] = b"DFIR-OGRE-ID";
const HASH_VERSION: u8 = 1;
const FIELD_TAG: u8 = 0xf0;

pub(crate) fn start(hasher: &mut Hasher, domain: &str) {
    hasher.update(HASH_PREFIX);
    hasher.update(&[HASH_VERSION]);
    write_bytes(hasher, domain.as_bytes());
}

pub(crate) fn write_tag(hasher: &mut Hasher, tag: u8) {
    hasher.update(&[tag]);
}

pub(crate) fn write_field(hasher: &mut Hasher, name: &str) {
    write_tag(hasher, FIELD_TAG);
    write_str(hasher, name);
}

pub(crate) fn write_bytes(hasher: &mut Hasher, value: &[u8]) {
    write_u64(hasher, value.len() as u64);
    hasher.update(value);
}

pub(crate) fn write_str(hasher: &mut Hasher, value: &str) {
    write_bytes(hasher, value.as_bytes());
}

pub(crate) fn write_option_str(hasher: &mut Hasher, value: Option<&str>) {
    match value {
        Some(value) => {
            write_tag(hasher, 1);
            write_str(hasher, value);
        }
        None => write_tag(hasher, 0),
    }
}

pub(crate) fn write_u64(hasher: &mut Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

pub(crate) fn write_i64(hasher: &mut Hasher, value: i64) {
    hasher.update(&value.to_le_bytes());
}

pub(crate) fn write_f64(hasher: &mut Hasher, value: f64) {
    write_u64(hasher, value.to_bits());
}

pub(crate) fn write_bool(hasher: &mut Hasher, value: bool) {
    write_tag(hasher, u8::from(value));
}

pub(crate) fn write_date(hasher: &mut Hasher, value: &DateTime<Utc>) {
    write_i64(hasher, value.timestamp());
    hasher.update(&value.timestamp_subsec_nanos().to_le_bytes());
}

pub(crate) fn write_option_date(hasher: &mut Hasher, value: Option<&DateTime<Utc>>) {
    match value {
        Some(value) => {
            write_tag(hasher, 1);
            write_date(hasher, value);
        }
        None => write_tag(hasher, 0),
    }
}
