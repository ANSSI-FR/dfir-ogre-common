use std::{fs, path::Path};

use dfir_nt_hive::{Hive, KeyValueDataType};

const KEY_VALUE_DATA_TYPE_OFFSET: usize = 12;
const HIVE_BASE_BLOCK_SIZE: usize = 4096;

pub(crate) fn write_oversized_dword_fixture(target: &Path, data_type: KeyValueDataType) -> Vec<u8> {
    assert!(matches!(
        data_type,
        KeyValueDataType::RegDWord | KeyValueDataType::RegDWordBigEndian
    ));

    let mut hive_bytes = fs::read("test_data/hive/SAM.hive").unwrap();
    let (data_type_offset, raw_data) = {
        let hive = Hive::without_validation(hive_bytes.as_slice()).unwrap();
        let key = hive
            .root_key_node()
            .unwrap()
            .subpath("SAM\\Domains\\Account\\Users\\000001F4")
            .unwrap()
            .unwrap();
        let value = key.value("F").unwrap().unwrap();

        assert_eq!(value.data_type().unwrap(), KeyValueDataType::RegBinary);
        assert_eq!(value.data_size(), 80);

        (
            HIVE_BASE_BLOCK_SIZE + value.header_range.start + KEY_VALUE_DATA_TYPE_OFFSET,
            value.data().unwrap().into_vec().unwrap(),
        )
    };

    hive_bytes[data_type_offset..data_type_offset + std::mem::size_of::<u32>()]
        .copy_from_slice(&(data_type as u32).to_le_bytes());
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, hive_bytes).unwrap();

    raw_data
}
