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
        self.writer.flush()?;
        self.writer.try_finish()?;
        Ok(())
    }
}
