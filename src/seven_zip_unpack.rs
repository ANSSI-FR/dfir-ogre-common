use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
};

use pyo3::prelude::*;
use sevenz_rust2::{Archive, BlockDecoder, Password};

use crate::Error;

fn safe_output_path(output_folder: &Path, output_name: &str) -> Result<PathBuf, Error> {
    if output_name.is_empty()
        || output_name.starts_with('/')
        || output_name.starts_with('\\')
        || output_name.contains(':')
    {
        return Err(Error::UnsafeArchivePath(output_name.to_owned()));
    }

    let mut path = output_folder.to_path_buf();
    for segment in output_name.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => return Err(Error::UnsafeArchivePath(output_name.to_owned())),
            segment => path.push(segment),
        }
    }

    Ok(path)
}

/// Holds a mapping from an archive entry path to the desired output filename.
#[derive(Debug, Default)]
#[pyclass]
pub struct FilesToExtract(HashMap<String, String>);

#[pymethods]
impl FilesToExtract {
    #[new]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Register a file to be extracted.
    ///
    /// * `input_path` – path of the entry inside the archive.
    /// * `output_path` – filename (relative to the output folder) where the entry should be written.
    pub fn add(&mut self, input_path: String, output_path: String) {
        self.0.insert(input_path, output_path);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Extract selected files from a 7‑zip archive.
///
/// * `archive_path` – path to the *.7z file.
/// * `files` – mapping of entries to extract.
/// * `output_folder` – directory where extracted files are written.
/// * `password` – optional password for encrypted archives.
#[pyfunction]
pub fn extract_7z_files(
    archive_path: &str,
    files: &FilesToExtract,
    output_folder: &str,
    password: Option<String>,
) -> Result<(), Error> {
    if files.0.is_empty() {
        return Ok(());
    }
    let mut file = File::open(archive_path)?;

    let password = match password {
        Some(pw) => Password::new(&pw),
        None => Password::empty(),
    };

    let archive = Archive::read(&mut file, &password)?;

    let block_count = archive.blocks.len();
    let output_folder = PathBuf::from(output_folder);
    let mut destinations = HashMap::with_capacity(files.0.len());
    for (entry_name, output_name) in &files.0 {
        destinations.insert(
            entry_name.as_str(),
            safe_output_path(&output_folder, output_name)?,
        );
    }

    for block_index in 0..block_count {
        let decoder = BlockDecoder::new(1, block_index, &archive, &password, &mut file);
        decoder.for_each_entries(&mut |entry, reader| {
            let entry_name = entry.name();

            // If the entry is requested, extract it to the destination path.
            // Otherwise, simply consume the data to keep the stream in sync.
            match destinations.get(entry_name) {
                Some(dest) => {
                    sevenz_rust2::default_entry_extract_fn(entry, reader, dest)?;
                }
                None => {
                    std::io::copy(reader, &mut std::io::sink())?;
                }
            };
            Ok(true)
        })?;
    }

    Ok(())
}

/// Extract selected files from a 7‑zip archive.
///
/// * `archive_path` – path to the *.7z file.
/// * `files` – mapping of entries to extract.
/// * `output_folder` – directory where extracted files are written.
/// * `password` – optional password for encrypted archives.
#[pyfunction]
pub fn extract_7z_file(
    archive_path: &str,
    filename: &str,
    output_folder: &str,
    password: Option<String>,
) -> Result<(), Error> {
    let mut file = File::open(archive_path)?;

    let password = match password {
        Some(pw) => Password::new(&pw),
        None => Password::empty(),
    };

    let archive = Archive::read(&mut file, &password)?;

    let block_count = archive.blocks.len();
    let output_folder = PathBuf::from(output_folder);
    let dest = safe_output_path(&output_folder, filename)?;

    for block_index in 0..block_count {
        let decoder = BlockDecoder::new(1, block_index, &archive, &password, &mut file);

        if !decoder
            .entries()
            .iter()
            .any(|entry| entry.name() == filename)
        {
            // skip the folder if it does not contain the file we want
            continue;
        }

        decoder.for_each_entries(&mut |entry, reader| {
            if entry.name() == filename {
                //only extract the file we want
                sevenz_rust2::default_entry_extract_fn(entry, reader, &dest)?;
            } else {
                //skip other files
                std::io::copy(reader, &mut std::io::sink())?;
            }
            Ok(true)
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};
    pub const TMP_FOLDER: &str = ".tmp";

    #[test]
    fn safe_output_path_rejects_escape_attempts() {
        let output_folder = Path::new("output");

        for output_name in [
            "../escaped.csv",
            "nested/../../escaped.csv",
            "/tmp/escaped.csv",
            "\\\\server\\share\\escaped.csv",
            "C:\\temp\\escaped.csv",
            "file.txt:stream",
        ] {
            assert!(
                matches!(
                    safe_output_path(output_folder, output_name),
                    Err(Error::UnsafeArchivePath(_))
                ),
                "expected {output_name:?} to be rejected"
            );
        }
    }

    #[test]
    fn safe_output_path_allows_nested_relative_paths() {
        let output_folder = Path::new("output");

        assert_eq!(
            safe_output_path(output_folder, "evtx/file.evtx").unwrap(),
            output_folder.join("evtx").join("file.evtx")
        );
        assert_eq!(
            safe_output_path(output_folder, "evtx\\file.evtx").unwrap(),
            output_folder.join("evtx").join("file.evtx")
        );
    }

    #[test]
    fn test_archive_with_password() {
        let out_dir = Path::new(TMP_FOLDER).join("7z");
        fs::create_dir_all(&out_dir).unwrap();

        // No files requested – we only care about the opening error.
        let mut files = FilesToExtract::default();
        files.add("input_path".to_string(), "output_path".to_string());

        let result = extract_7z_files(
            "nonexistent_archive.7z",
            &files,
            out_dir.to_str().unwrap(),
            Some("password".to_owned()),
        );

        match result {
            Err(Error::Io(_)) => {} // Expected path‑not‑found error.
            other => panic!("expected Io error, got {:?}", other),
        }

        let result = extract_7z_files(
            "test_data/7z/secret.7z",
            &files,
            out_dir.to_str().unwrap(),
            Some("wrong_password".to_owned()),
        );
        match result {
            Err(Error::SevenZip(_)) => {} // Expected MaybeBadPassword error.
            other => panic!("expected SevenZip error, got {:?}", other),
        }

        extract_7z_files(
            "test_data/7z/secret.7z",
            &files,
            out_dir.to_str().unwrap(),
            Some("password".to_owned()),
        )
        .unwrap();
    }

    #[test]
    fn test_extract_files() {
        let out_dir = Path::new(TMP_FOLDER).join("7z").join("extract");
        if fs::exists(&out_dir).unwrap() {
            fs::remove_dir_all(&out_dir).unwrap();
        }

        fs::create_dir_all(&out_dir).unwrap();

        let mut files = FilesToExtract::default();
        files.add("GetThis.csv".to_owned(), "GetThis.csv".to_owned());
        files.add("evtx/EBE79D4BE79B4B5_1000000001173_3000000000030_4_Microsoft-Windows-WinRM%4Operational.evtx_{00000000-0000-0000-0000-000000000000}.data".to_owned(), "evtx/WinRM%4Operationa.data".to_owned());
        files.add("evtx/EBE79D4BE79B4B5_1000000001173_2000000000032_4_Microsoft-Windows-Kernel-EventTracing%4Admin.evtx_{00000000-0000-0000-0000-000000000000}.data".to_owned(), "evtx/EBE79D4BE79B4B5_1000000001173_2000000000032_4_Microsoft-Windows-Kernel-EventTracing%4Admin.evtx_{00000000-0000-0000-0000-000000000000}.data".to_owned());

        extract_7z_files(
            "test_data/7z/Event.7z",
            &files,
            out_dir.to_str().unwrap(),
            Some("password".to_owned()),
        )
        .unwrap();

        assert!(fs::exists(&out_dir.join("GetThis.csv")).unwrap());
        assert!(fs::exists(&out_dir.join("evtx/WinRM%4Operationa.data")).unwrap());
        assert!(fs::exists(&out_dir.join("evtx/EBE79D4BE79B4B5_1000000001173_2000000000032_4_Microsoft-Windows-Kernel-EventTracing%4Admin.evtx_{00000000-0000-0000-0000-000000000000}.data")).unwrap());
    }

    #[test]
    fn extract_7z_files_rejects_output_path_traversal() {
        let base_dir = Path::new(TMP_FOLDER).join("7z").join("traversal");
        if fs::exists(&base_dir).unwrap() {
            fs::remove_dir_all(&base_dir).unwrap();
        }

        let out_dir = base_dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();
        let escaped_path = base_dir.join("escaped.csv");

        let mut files = FilesToExtract::default();
        files.add("GetThis.csv".to_owned(), "../escaped.csv".to_owned());

        let result = extract_7z_files(
            "test_data/7z/Event.7z",
            &files,
            out_dir.to_str().unwrap(),
            Some("password".to_owned()),
        );

        assert!(
            matches!(result, Err(Error::UnsafeArchivePath(_))),
            "expected traversal path to be rejected"
        );
        assert!(!fs::exists(&escaped_path).unwrap());
    }

    #[test]
    fn extract_7z_file_rejects_archive_entry_path_traversal() {
        let base_dir = Path::new(TMP_FOLDER).join("7z").join("single_traversal");
        if fs::exists(&base_dir).unwrap() {
            fs::remove_dir_all(&base_dir).unwrap();
        }

        let out_dir = base_dir.join("out");
        fs::create_dir_all(&out_dir).unwrap();
        let escaped_path = base_dir.join("escaped.csv");

        let result = extract_7z_file(
            "test_data/7z/Event.7z",
            "../escaped.csv",
            out_dir.to_str().unwrap(),
            Some("password".to_owned()),
        );

        assert!(
            matches!(result, Err(Error::UnsafeArchivePath(_))),
            "expected traversal path to be rejected"
        );
        assert!(!fs::exists(&escaped_path).unwrap());
    }
}
