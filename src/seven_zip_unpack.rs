use std::{collections::HashMap, fs::File, path::PathBuf};

use pyo3::prelude::*;
use sevenz_rust2::{Archive, BlockDecoder, Password};

use crate::Error;

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
    for block_index in 0..block_count {
        let decoder = BlockDecoder::new(1, block_index, &archive, &password, &mut file);
        decoder.for_each_entries(&mut |entry, reader| {
            let entry_name = entry.name();

            // If the entry is requested, extract it to the destination path.
            // Otherwise, simply consume the data to keep the stream in sync.
            match files.0.get(entry_name) {
                Some(output_name) => {
                    let dest = output_folder.join(output_name);
                    sevenz_rust2::default_entry_extract_fn(entry, reader, &dest)?;
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
        let output_folder = PathBuf::from(output_folder);

        decoder.for_each_entries(&mut |entry, reader| {
            if entry.name() == filename {
                //only extract the file we want
                let dest = output_folder.join(entry.name());
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
}
