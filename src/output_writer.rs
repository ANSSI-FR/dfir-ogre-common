use flate2::{Compression, write::GzEncoder};
use std::{
    fs::File,
    io::{BufWriter, Write},
};

use crate::errors::Error;

const BUFFER_SIZE: usize = 5 * 1024 * 1024;

/// A trait defining the interface for writing data to a destination.
/// Implementations of this trait are responsible for writing data and properly closing the output stream.
pub trait Writer {
    /// Writes the provided buffer to the output destination.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to a string buffer containing the data to write.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the write operation was successful.
    /// * `Err(Error)` if the write operation failed (e.g., due to I/O errors).
    fn write(&mut self, buf: &mut String) -> Result<(), Error>;

    /// Closes the writer and flushes any remaining data to the output destination.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the stream was successfully closed.
    /// * `Err(Error)` if an error occurred during closing (e.g., due to I/O issues).
    fn close(&mut self) -> Result<(), Error>;
}

/// Attempt to close every item, even after one close fails.
///
/// The first error is returned after all close operations have been attempted.
pub(crate) fn try_close_all<'a, T: ?Sized + 'a>(
    items: impl IntoIterator<Item = &'a mut T>,
    mut close: impl FnMut(&mut T) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut first_error = None;
    for item in items {
        if let Err(error) = close(item)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// A concrete implementation of the `Writer` trait that writes data to a file.
/// Uses a buffered writer to improve performance.
pub struct FileWriter {
    /// The underlying buffered writer that handles the actual file I/O.
    writer: BufWriter<File>,
}
impl FileWriter {
    pub fn new(file: File) -> Self {
        let writer = BufWriter::with_capacity(BUFFER_SIZE, file);
        Self { writer }
    }
}
impl Writer for FileWriter {
    fn write(&mut self, buf: &mut String) -> Result<(), Error> {
        self.writer.write_all(buf.as_bytes())?;
        Ok(())
    }

    fn close(&mut self) -> Result<(), Error> {
        self.writer.flush()?;
        Ok(())
    }
}

/// A concrete implementation of the `Writer` trait that writes compressed data to a file using gzip compression.
/// The writer wraps a buffered file writer with a gzip encoder to compress data on-the-fly before writing.
pub struct GzipWriter {
    writer: GzEncoder<BufWriter<File>>,
}

impl GzipWriter {
    /// # Arguments
    ///
    /// * `file` - The file to write compressed data to. The file must be opened for writing.
    /// * `level` - The compression level to use, ranging from 1 (fast, and low compression ratio) to 9 (slow but maximum compression).
    ///   A value of 6 is typically a good balance between speed and compression ratio.
    ///
    pub fn new(file: File, level: u32) -> Self {
        let writer = GzEncoder::new(
            BufWriter::with_capacity(BUFFER_SIZE, file),
            Compression::new(level),
        );
        Self { writer }
    }
}
impl Writer for GzipWriter {
    fn write(&mut self, buf: &mut String) -> Result<(), Error> {
        self.writer.write_all(buf.as_bytes())?;
        Ok(())
    }

    fn close(&mut self) -> Result<(), Error> {
        self.writer.try_finish()?;
        self.writer.get_mut().flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::{
        fs::{self, File},
        io::Read,
        path::PathBuf,
    };

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dfir_ogre_common_{name}_{}", std::process::id()))
    }

    #[test]
    fn file_writer_writes_buffer_when_closed() {
        let path = temp_file("file_writer.txt");
        let file = File::create(&path).unwrap();
        let mut writer = FileWriter::new(file);
        let mut buffer = "first line\nsecond line".to_string();

        writer.write(&mut buffer).unwrap();
        writer.close().unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "first line\nsecond line"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn gzip_writer_finishes_readable_gzip_stream_when_closed() {
        let path = temp_file("gzip_writer.gz");
        let file = File::create(&path).unwrap();
        let mut writer = GzipWriter::new(file, 6);
        let mut buffer = "compressed payload".to_string();

        writer.write(&mut buffer).unwrap();
        writer.close().unwrap();

        let mut decoder = GzDecoder::new(File::open(&path).unwrap());
        let mut decoded = String::new();
        decoder.read_to_string(&mut decoded).unwrap();

        assert_eq!(decoded, "compressed payload");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn try_close_all_attempts_every_item_and_returns_first_error() {
        struct CloseTarget {
            name: &'static str,
            should_fail: bool,
            attempted: bool,
        }

        let mut targets = [
            CloseTarget {
                name: "first",
                should_fail: true,
                attempted: false,
            },
            CloseTarget {
                name: "second",
                should_fail: false,
                attempted: false,
            },
            CloseTarget {
                name: "third",
                should_fail: true,
                attempted: false,
            },
        ];

        let error = try_close_all(&mut targets, |target| {
            target.attempted = true;
            if target.should_fail {
                Err(Error::PluginError(target.name.to_owned()))
            } else {
                Ok(())
            }
        })
        .unwrap_err();

        assert!(matches!(error, Error::PluginError(message) if message == "first"));
        assert!(targets.iter().all(|target| target.attempted));
    }
}
