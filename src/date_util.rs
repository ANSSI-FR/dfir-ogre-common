use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

use crate::errors::Error;
use pyo3::prelude::*;

const FILETIME_TICKS_PER_SECOND: i128 = 10_000_000;
const FILETIME_UNIX_EPOCH_TICKS: i128 = 116_444_736_000_000_000;

pub(crate) fn filetime_to_datetime(timestamp: u64) -> Result<DateTime<Utc>, Error> {
    let unix_ticks = i128::from(timestamp) - FILETIME_UNIX_EPOCH_TICKS;
    let seconds = unix_ticks.div_euclid(FILETIME_TICKS_PER_SECOND);
    let subsecond_ticks = unix_ticks.rem_euclid(FILETIME_TICKS_PER_SECOND);
    let seconds = i64::try_from(seconds).map_err(|_| Error::InvalidTimestamp())?;
    let nanoseconds =
        u32::try_from(subsecond_ticks * 100).map_err(|_| Error::InvalidTimestamp())?;

    Utc.timestamp_opt(seconds, nanoseconds)
        .single()
        .ok_or(Error::InvalidTimestamp())
}

/// Defines supported formats for parsing date strings
#[derive(Debug, Clone)]
#[pyclass(from_py_object)]
pub enum DateInputCodec {
    ///A codec that parses date strings in ISO 8601 format (e.g., '2023-12-31T23:59:59').
    Iso(),

    //A codec that parses date strings as Windows File Time in seconds (e.g., 133210364868558102).
    FileTime(),

    ///A codec that parses date strings as Unix timestamps in seconds (e.g., 1703654400).
    Timestamp(),

    ///A codec that parses date strings in  Unix timestamps in seconds, in hexadecimal format as (e.g., 0x5010ad0a).
    TimestampHex(),

    ///A codec that parses date strings as Unix timestamps in milliseconds (e.g., 1704067199000).
    TimestampMs(),

    ///A codec that parses date strings as Unix timestamps in nanosecond (e.g., 1703664000123456789).
    TimestampNs(),

    ///A codec that parses date strings using the given format pattern (e.g., '%Y-%m-%d %H:%M:%S')."""
    Pattern(String),
}

impl DateInputCodec {
    /// Creates a `DateInputCodec` instance from a string name.
    ///
    /// This inherent constructor is kept for existing Rust callers. New code can
    /// use `DateInputCodec::from(name)` or `name.parse::<DateInputCodec>()`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(name: &str) -> Self {
        Self::from(name)
    }
}

impl From<&str> for DateInputCodec {
    /// Creates a `DateInputCodec` instance from a string name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the codec. Supported values are:
    ///   - `"iso"` for ISO 8601 format.
    ///   - `"timestamp_ms"` for Unix timestamps in milliseconds.
    ///   - Any other string is treated as a custom format pattern (e.g., `"%Y-%m-%d %H:%M:%S"`).
    ///
    /// # Returns
    ///
    /// A `DateInputCodec` variant corresponding to the provided name.
    ///
    fn from(name: &str) -> Self {
        match name {
            "iso" => DateInputCodec::Iso(),
            "file_time" => DateInputCodec::FileTime(),
            "timestamp" => DateInputCodec::Timestamp(),
            "timestamp_hex" => DateInputCodec::TimestampHex(),
            "timestamp_ms" => DateInputCodec::TimestampMs(),
            "timestamp_ns" => DateInputCodec::TimestampNs(),
            _ => DateInputCodec::Pattern(name.to_owned()),
        }
    }
}

impl std::str::FromStr for DateInputCodec {
    type Err = std::convert::Infallible;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(name))
    }
}

/// Parses a date string into a `DateTime<Utc>` using the provided codec.
///
/// # Arguments
///
/// * `input` - The date string to parse.
/// * `codec` - The codec to use for parsing the date string.
///
/// # Returns
///
/// A `Result<DateTime<Utc>, Error>` containing the parsed date in UTC, or an error if parsing fails.
///
/// # Errors
///
/// Returns `Error::InvalidTimestamp` if the input is a timestamp but cannot be parsed or is out of range.
/// Returns `Error::ParseError` if the input string cannot be parsed according to the specified format.
pub fn parse_date(input: &str, codec: &DateInputCodec) -> Result<DateTime<Utc>, Error> {
    match codec {
        DateInputCodec::Iso() => Ok(DateTime::parse_from_rfc3339(input)?.to_utc()),
        DateInputCodec::Pattern(name) => {
            if name.contains("%z") {
                Ok(DateTime::parse_from_str(input, name)?.to_utc())
            } else {
                let parsed = NaiveDateTime::parse_from_str(input, name)?;
                let dt: DateTime<Utc> = parsed.and_utc();
                Ok(dt)
            }
        }
        DateInputCodec::Timestamp() => {
            let timestamp = input.parse::<i64>()?;
            Utc.timestamp_opt(timestamp, 0)
                .single()
                .ok_or(Error::InvalidTimestamp())
        }

        DateInputCodec::TimestampHex() => {
            let timestamp = if input.starts_with("0x") {
                let inp = input.replacen("0x", "", 1);
                i64::from_str_radix(&inp, 16)?
            } else {
                i64::from_str_radix(input, 16)?
            };

            Utc.timestamp_opt(timestamp, 0)
                .single()
                .ok_or(Error::InvalidTimestamp())
        }

        DateInputCodec::TimestampMs() => {
            let timestamp = input.parse::<i64>()?;
            Utc.timestamp_millis_opt(timestamp)
                .single()
                .ok_or(Error::InvalidTimestamp())
        }
        DateInputCodec::TimestampNs() => {
            let timestamp = input.parse::<i64>()?;
            Ok(Utc.timestamp_nanos(timestamp))
        }
        DateInputCodec::FileTime() => {
            let timestamp = input.parse::<u64>()?;
            filetime_to_datetime(timestamp)
        }
    }
}
/// Defines supported formats for serializing date objects
#[derive(Debug, Clone)]
#[pyclass(from_py_object)]
pub enum DateOutputCodec {
    /// Serializes the date in ISO 8601 format with timezone offset (e.g., '2023-12-31T23:59:59.123-08:00').
    Iso(),

    /// Serializes the date in ISO 8601 format in UTC (e.g., '2023-12-31T23:59:59.123+00:00').
    IsoUtc(),

    /// Serializes the date as a naive UTC datetime string (e.g., '2023-12-31 23:59:59.123').
    UtcNaive(),

    /// Serializes the date using a custom format pattern (e.g., '%Y-%m-%d %H:%M:%S').
    Pattern(String),
}

impl DateOutputCodec {
    /// Creates a `DateOutputCodec` instance from a string name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the codec. Supported values are:
    ///   - `"iso"` for ISO 8601 format with timezone offset.
    ///   - `"iso_utc"` for ISO 8601 format in UTC (Z suffix).
    ///   - `"utc_naive"` for naive UTC datetime string (no timezone).
    ///   - Any other string is treated as a custom format pattern (e.g., `"%Y-%m-%d %H:%M:%S"`).
    ///
    /// # Returns
    ///
    /// A `DateOutputCodec` variant corresponding to the provided name.
    ///
    pub fn from_string(name: &str) -> Self {
        match name {
            "iso" => DateOutputCodec::Iso(),
            "iso_utc" => DateOutputCodec::IsoUtc(),
            "utc_naive" => DateOutputCodec::UtcNaive(),
            _ => DateOutputCodec::Pattern(name.to_owned()),
        }
    }
}

/// Serializes a `DateTime<Utc>` into a formatted string using the specified codec.
///
/// # Arguments
///
/// * `date` - The date to serialize.
/// * `date_format` - The codec to use for formatting the date.
///
/// # Returns
///
/// A `String` containing the formatted date representation.
///
pub fn serialize_date(date: &DateTime<Utc>, date_format: &DateOutputCodec) -> String {
    match date_format {
        DateOutputCodec::Iso() => date.to_utc().format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string(),
        DateOutputCodec::IsoUtc() => date.to_utc().format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string(),
        DateOutputCodec::UtcNaive() => date.naive_utc().format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
        DateOutputCodec::Pattern(pattern) => date.format(pattern).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This test verifies that:
    /// 1. An ISO formatted input string is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    ///    - Naive UTC datetime string
    ///    - Custom pattern string
    #[test]
    fn date_iso() {
        let codec = DateInputCodec::from("iso");

        let input = "1996-12-19T16:39:57.123-08:00";
        let date = parse_date(input, &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123000+00:00");

        let output_codec = DateOutputCodec::from_string("iso_utc");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123000+00:00");

        let output_codec = DateOutputCodec::from_string("utc_naive");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.123000");

        let serialized = format!("{}", date.format("%Y-%m-%d %H:%M:%S.%3f"));
        assert_eq!(serialized, "1996-12-20 00:39:57.123");

        let output_codec = DateOutputCodec::from_string("%Y-%m-%d %H:%M:%S.%3f");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.123");
    }

    /// 1. A naive date string with microseconds is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    #[test]
    fn date_naive() {
        let codec = DateInputCodec::from("%Y-%m-%d %H:%M:%S.%3f");

        let input = "2016-01-22 03:08:51.337";
        let date = parse_date(input, &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");

        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "2016-01-22T03:08:51.337000+00:00");

        let output_codec = DateOutputCodec::from_string("iso_utc");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "2016-01-22T03:08:51.337000+00:00");
    }

    /// This test ensures that:
    /// 1. A date string with microseconds and timezone offset is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    #[test]
    fn date_custom() {
        let codec = DateInputCodec::from("%Y-%m-%d %H:%M:%S.%3f %z");

        let input = "1996-12-19 16:39:57.123 -08:00";
        let date = parse_date(input, &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123000+00:00");

        let output_codec = DateOutputCodec::from_string("iso_utc");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123000+00:00");
    }

    /// 1. A Unix timestamp in seconds is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    ///    - Naive UTC datetime string
    ///    - Custom pattern string
    #[test]
    fn timestamp() {
        let codec = DateInputCodec::from("iso");

        let input = "1996-12-19T16:39:57.123-08:00";
        let date = parse_date(input, &codec).unwrap();

        let ts = date.timestamp();
        let codec = DateInputCodec::from("timestamp");

        let date = parse_date(&ts.to_string(), &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.000000+00:00");

        let output_codec = DateOutputCodec::from_string("iso_utc");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.000000+00:00");

        let output_codec = DateOutputCodec::from_string("utc_naive");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.000000");

        let output_codec = DateOutputCodec::from_string("%Y-%m-%d %H:%M:%S.%3f");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.000");
    }

    /// 1. A Unix timestamp in milliseconds is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    ///    - Naive UTC datetime string
    ///    - Custom pattern string
    #[test]
    fn timestamp_ms() {
        let codec = DateInputCodec::from("iso");

        let input = "1996-12-19T16:39:57.123-08:00";
        let date = parse_date(input, &codec).unwrap();

        let ts = date.timestamp_millis();
        let codec = DateInputCodec::from("timestamp_ms");

        let date = parse_date(&ts.to_string(), &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123000+00:00");

        let output_codec = DateOutputCodec::from_string("iso_utc");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123000+00:00");

        let output_codec = DateOutputCodec::from_string("utc_naive");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.123000");

        let output_codec = DateOutputCodec::from_string("%Y-%m-%d %H:%M:%S.%3f");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.123");
    }

    /// 1. A Unix timestamp in seconds is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    ///    - Naive UTC datetime string
    ///    - Custom pattern string
    #[test]
    fn timestamp_ns() {
        let codec = DateInputCodec::from("iso");

        let input = "1996-12-19T16:39:57.123456-08:00";
        let date = parse_date(input, &codec).unwrap();

        let ts = date.timestamp_nanos_opt().unwrap();
        let codec = DateInputCodec::from("timestamp_ns");

        let date = parse_date(&ts.to_string(), &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123456+00:00");

        let output_codec = DateOutputCodec::from_string("iso_utc");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20T00:39:57.123456+00:00");

        let output_codec = DateOutputCodec::from_string("utc_naive");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.123456");

        let output_codec = DateOutputCodec::from_string("%Y-%m-%d %H:%M:%S.%6f");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "1996-12-20 00:39:57.123456");
    }

    /// 1. A Unix timestamp in seconds is correctly parsed into a DateTime<Utc>.
    /// 2. The resulting date can be serialized back to different output formats:
    ///    - ISO with timezone offset
    ///    - ISO UTC format
    ///    - Naive UTC datetime string
    ///    - Custom pattern string
    #[test]
    fn file_time() {
        let codec = DateInputCodec::from("file_time");

        let input = "133210364868558102";
        let first_date = parse_date(input, &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&first_date, &output_codec);
        assert_eq!(serialized, "2023-02-16T15:54:46.855810+00:00");
        assert_eq!(first_date.timestamp_subsec_nanos(), 855_810_200);

        let adjacent = parse_date("133210364868558103", &codec).unwrap();
        assert_eq!(
            adjacent.timestamp_nanos_opt().unwrap() - first_date.timestamp_nanos_opt().unwrap(),
            100
        );

        let input = "132723834270000000";
        let date = parse_date(input, &codec).unwrap();

        let output_codec = DateOutputCodec::from_string("iso");
        let serialized = serialize_date(&date, &output_codec);
        assert_eq!(serialized, "2021-08-02T13:10:27.000000+00:00");
    }

    #[test]
    fn timestamp_hex_accepts_prefixed_and_plain_values() {
        let codec = DateInputCodec::from("timestamp_hex");

        let prefixed = parse_date("0x5010ad0a", &codec).unwrap();
        let plain = parse_date("5010ad0a", &codec).unwrap();

        assert_eq!(prefixed, plain);
        assert_eq!(prefixed.timestamp(), 0x5010ad0a);
        assert_eq!(
            serialize_date(&prefixed, &DateOutputCodec::from_string("iso")),
            "2012-07-26T02:35:54.000000+00:00"
        );
    }
}
