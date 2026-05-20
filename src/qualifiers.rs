use std::collections::HashMap;

use pyo3::prelude::*;

use crate::Error;
#[allow(non_snake_case)]
/// Struct to store metadata attributes for system events, file properties, and contextual data.
/// Organized into categories like timestamps, computer details, OS info, file system data, network attributes, etc.
#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct Qualifiers {
    // Timestamp
    pub DATE_CREATION: String,
    pub DATE_MODIFICATION: String,
    pub DATE_CHANGE: String,
    pub DATE_ACCESS: String,
    pub DATE_COMPILATION: String,
    pub DATE_INSTALLATION: String,
    pub DATE_UNINSTALL: String,
    pub DATE_LAST_RUN: String,
    pub TIMEZONE: String,

    // Computer;
    pub COMPUTER_NAME: String,

    // OS;
    pub OS_VERSION: String,
    pub OS_ARCH: String,

    // User
    pub USER_NAME: String,
    pub USER_SID: String,
    pub USER_ID: String,
    pub LOGON_ID: String,

    // Group
    pub GROUP_ID: String,
    pub GROUP_NAME: String,

    // Filesystem
    pub FS_INODE: String,
    pub FS_USN: String,
    pub VOLUME_GUID: String,
    pub MFT_SEQUENCE: String,

    // Disk
    pub DISK_SIZE: String,

    // File
    pub FILE_NAME: String,
    pub FILE_SIZE: String,
    pub FILE_PATH: String,
    pub FILE_PATH_SHA1: String,
    pub FILE_MD5: String,
    pub FILE_SHA1: String,
    pub FILE_SHA256: String,
    pub FILE_SHA384: String,
    pub FILE_SHA512: String,
    pub FILE_TIGER: String,
    pub FILE_WHIRLPOOL: String,
    pub FILE_SSDEEP: String,
    pub FILE_TLSH: String,
    pub FILE_ATTRS: String,

    // PE
    pub PE_MD5: String,
    pub PE_SHA1: String,
    pub PE_SHA256: String,
    pub PE_ARCH: String,
    pub PE_SUBSYSTEM: String,
    pub PE_VERSION: String,
    pub EXIT_CODE: String,

    // File execution
    pub COMMAND_LINE: String,

    // Application
    pub APP_ID: String,
    pub APP_NAME: String,
    pub APP_CLSID: String,
    pub MSI_PRODUCT: String,
    pub MSI_PACKAGE: String,

    // Publisher;
    pub COMPANY: String,
    pub PUBLISHER: String,
    pub PRODUCT: String,

    // Certificate;
    pub CERT_SHA1: String,

    // Registry;
    pub HIVE_MOUNT: String,
    pub KEY_NAME: String,
    pub KEY_PATH: String,
    pub VALUE_NAME: String,
    pub VALUE_DATA: String,

    // Service;
    pub SERVICE_NAME: String,
    pub SERVICE_TYPE: String,
    pub SERVICE_DISPLAY_NAME: String,
    pub SERVICE_START_TYPE: String,

    // Process;
    pub PROCESS_ID: String,

    // ScheduledTask;
    pub SCHTASK_GUID: String,
    pub SCHTASK_URI: String,

    // Event;
    pub EVT_PROVIDER: String,
    pub EVT_ID: String,
    pub EVT_CHANNEL: String,
    pub EVT_RECORD_ID: String,

    // State;
    pub IN_USE: String, // qualifies a boolean, used to tell deleted objects apart from existing ones.to_owned();
    pub REUSE_COUNT: String, // qualifies an int, used to tell how many times an object has been reused.to_owned();

    // DNS;
    pub DOMAIN_NAME: String,

    // Windows;
    pub WINDOWS_PRIVILEGES: String,
    pub SECURITY_DESCRIPTOR: String,
    pub WINDOWS_OBJECT: String,

    // Network;
    pub IP_ADDRESS: String,
    pub IP_PORT: String,
    pub MAC_ADDRESS: String,

    map: HashMap<String, String>,
}
#[pymethods]
impl Qualifiers {
    #[new]
    pub fn new() -> Self {
        let mut map = HashMap::new();

        map.insert("DATE_CREATION".to_owned(), "creation_date".to_owned());
        map.insert(
            "DATE_MODIFICATION".to_owned(),
            "modification_date".to_owned(),
        );
        map.insert("DATE_CHANGE".to_owned(), "change_date".to_owned());
        map.insert("DATE_ACCESS".to_owned(), "access_date".to_owned());
        map.insert("DATE_COMPILATION".to_owned(), "compilation_date".to_owned());
        map.insert(
            "DATE_INSTALLATION".to_owned(),
            "installation_date".to_owned(),
        );
        map.insert("DATE_UNINSTALL".to_owned(), "uninstall_date".to_owned());
        map.insert("DATE_LAST_RUN".to_owned(), "last_run_date".to_owned());
        map.insert("TIMEZONE".to_owned(), "timezone".to_owned());

        // Computer
        map.insert("COMPUTER_NAME".to_owned(), "computer_name".to_owned());

        // OS
        map.insert("OS_VERSION".to_owned(), "os_version".to_owned());
        map.insert("OS_ARCH".to_owned(), "os_arch".to_owned());

        // User
        map.insert("USER_NAME".to_owned(), "user_name".to_owned());
        map.insert("USER_SID".to_owned(), "user_sid".to_owned());
        map.insert("USER_ID".to_owned(), "user_id".to_owned());
        map.insert("LOGON_ID".to_owned(), "logon_id".to_owned());

        // Group
        map.insert("GROUP_ID".to_owned(), "group_id".to_owned());
        map.insert("GROUP_NAME".to_owned(), "group_name".to_owned());

        // Filesystem
        map.insert("FS_INODE".to_owned(), "fs_inode".to_owned());
        map.insert("FS_USN".to_owned(), "usn".to_owned());
        map.insert("VOLUME_GUID".to_owned(), "volume_guid".to_owned());
        map.insert("MFT_SEQUENCE".to_owned(), "mft_sequence".to_owned());

        // Disk
        map.insert("DISK_SIZE".to_owned(), "disk_size".to_owned());

        // File
        map.insert("FILE_NAME".to_owned(), "file_name".to_owned());
        map.insert("FILE_SIZE".to_owned(), "file_size".to_owned());
        map.insert("FILE_PATH".to_owned(), "file_path".to_owned());
        map.insert("FILE_PATH_SHA1".to_owned(), "path_sha1".to_owned());
        map.insert("FILE_MD5".to_owned(), "file_md5".to_owned());
        map.insert("FILE_SHA1".to_owned(), "file_sha1".to_owned());
        map.insert("FILE_SHA256".to_owned(), "file_sha256".to_owned());
        map.insert("FILE_SHA384".to_owned(), "file_sha384".to_owned());
        map.insert("FILE_SHA512".to_owned(), "file_sha512".to_owned());
        map.insert("FILE_TIGER".to_owned(), "file_tiger".to_owned());
        map.insert("FILE_WHIRLPOOL".to_owned(), "file_whirlpool".to_owned());
        map.insert("FILE_SSDEEP".to_owned(), "file_ssdeep".to_owned());
        map.insert("FILE_TLSH".to_owned(), "file_tlsh".to_owned());
        map.insert("FILE_ATTRS".to_owned(), "file_attrs".to_owned());

        // PE
        map.insert("PE_MD5".to_owned(), "pe_md5".to_owned());
        map.insert("PE_SHA1".to_owned(), "pe_sha1".to_owned());
        map.insert("PE_SHA256".to_owned(), "pe_sha256".to_owned());
        map.insert("PE_ARCH".to_owned(), "pe_arch".to_owned());
        map.insert("PE_SUBSYSTEM".to_owned(), "pe_subsystem".to_owned());
        map.insert("PE_VERSION".to_owned(), "version".to_owned());
        map.insert("EXIT_CODE".to_owned(), "exit_code".to_owned());

        // File execution
        map.insert("COMMAND_LINE".to_owned(), "command_line".to_owned());

        // Application
        map.insert("APP_ID".to_owned(), "app_id".to_owned());
        map.insert("APP_NAME".to_owned(), "app_name".to_owned());
        map.insert("APP_CLSID".to_owned(), "app_clsid".to_owned());
        map.insert("MSI_PRODUCT".to_owned(), "msi_product".to_owned());
        map.insert("MSI_PACKAGE".to_owned(), "msi_package".to_owned());

        // Publisher
        map.insert("COMPANY".to_owned(), "company_name".to_owned());
        map.insert("PUBLISHER".to_owned(), "publisher_name".to_owned());
        map.insert("PRODUCT".to_owned(), "product_name".to_owned());

        // Certificate
        map.insert("CERT_SHA1".to_owned(), "cert_sha1".to_owned());

        // Registry
        map.insert("HIVE_MOUNT".to_owned(), "hive_mount".to_owned());
        map.insert("KEY_NAME".to_owned(), "key_name".to_owned());
        map.insert("KEY_PATH".to_owned(), "key_path".to_owned());
        map.insert("VALUE_NAME".to_owned(), "value_name".to_owned());
        map.insert("VALUE_DATA".to_owned(), "value_data".to_owned());

        // Service
        map.insert("SERVICE_NAME".to_owned(), "service_name".to_owned());
        map.insert("SERVICE_TYPE".to_owned(), "service_type".to_owned());
        map.insert(
            "SERVICE_DISPLAY_NAME".to_owned(),
            "service_display_name".to_owned(),
        );
        map.insert(
            "SERVICE_START_TYPE".to_owned(),
            "service_start_type".to_owned(),
        );

        // Process
        map.insert("PROCESS_ID".to_owned(), "process_id".to_owned());

        // ScheduledTask
        map.insert("SCHTASK_GUID".to_owned(), "schtask_guid".to_owned());
        map.insert("SCHTASK_URI".to_owned(), "schtask_uri".to_owned());

        // Event
        map.insert("EVT_PROVIDER".to_owned(), "evt_provider".to_owned());
        map.insert("EVT_ID".to_owned(), "evt_id".to_owned());
        map.insert("EVT_CHANNEL".to_owned(), "evt_channel".to_owned());
        map.insert("EVT_RECORD_ID".to_owned(), "evt_record_id".to_owned());

        // State
        map.insert("IN_USE".to_owned(), "in_use".to_owned());
        map.insert("REUSE_COUNT".to_owned(), "reuse_count".to_owned());

        // DNS
        map.insert("DOMAIN_NAME".to_owned(), "domain_name".to_owned());

        // Windows
        map.insert(
            "WINDOWS_PRIVILEGES".to_owned(),
            "windows_privileges".to_owned(),
        );
        map.insert(
            "SECURITY_DESCRIPTOR".to_owned(),
            "security_descriptor".to_owned(),
        );
        map.insert("WINDOWS_OBJECT".to_owned(), "windows_object".to_owned());

        // Network
        map.insert("IP_ADDRESS".to_owned(), "ip_address".to_owned());
        map.insert("IP_PORT".to_owned(), "ip_port".to_owned());
        map.insert("MAC_ADDRESS".to_owned(), "mac_address".to_owned());

        Self {
            map: map.clone(),
            // Timestamp
            DATE_CREATION: map.get("DATE_CREATION").unwrap().to_owned(),
            DATE_MODIFICATION: map.get("DATE_MODIFICATION").unwrap().to_owned(),
            DATE_CHANGE: map.get("DATE_CHANGE").unwrap().to_owned(),
            DATE_ACCESS: map.get("DATE_ACCESS").unwrap().to_owned(),
            DATE_COMPILATION: map.get("DATE_COMPILATION").unwrap().to_owned(),
            DATE_INSTALLATION: map.get("DATE_INSTALLATION").unwrap().to_owned(),
            DATE_UNINSTALL: map.get("DATE_UNINSTALL").unwrap().to_owned(),
            DATE_LAST_RUN: map.get("DATE_LAST_RUN").unwrap().to_owned(),
            TIMEZONE: map.get("TIMEZONE").unwrap().to_owned(),

            // Computer,
            COMPUTER_NAME: map.get("COMPUTER_NAME").unwrap().to_owned(),

            // OS,
            OS_VERSION: map.get("OS_VERSION").unwrap().to_owned(),
            OS_ARCH: map.get("OS_ARCH").unwrap().to_owned(),

            // User.to_owned(),
            USER_NAME: map.get("USER_NAME").unwrap().to_owned(),
            USER_SID: map.get("USER_SID").unwrap().to_owned(),
            USER_ID: map.get("USER_ID").unwrap().to_owned(),
            LOGON_ID: map.get("LOGON_ID").unwrap().to_owned(),

            // Group.to_owned(),
            GROUP_ID: map.get("GROUP_ID").unwrap().to_owned(),
            GROUP_NAME: map.get("GROUP_NAME").unwrap().to_owned(),

            // Filesystem.to_owned(),
            FS_INODE: map.get("FS_INODE").unwrap().to_owned(),
            FS_USN: map.get("FS_USN").unwrap().to_owned(),
            VOLUME_GUID: map.get("VOLUME_GUID").unwrap().to_owned(),
            MFT_SEQUENCE: map.get("MFT_SEQUENCE").unwrap().to_owned(),

            // Disk.to_owned(),
            DISK_SIZE: map.get("DISK_SIZE").unwrap().to_owned(),

            // File.to_owned(),
            FILE_NAME: map.get("FILE_NAME").unwrap().to_owned(),
            FILE_SIZE: map.get("FILE_SIZE").unwrap().to_owned(),
            FILE_PATH: map.get("FILE_PATH").unwrap().to_owned(),
            FILE_PATH_SHA1: map.get("FILE_PATH_SHA1").unwrap().to_owned(),
            FILE_MD5: map.get("FILE_MD5").unwrap().to_owned(),
            FILE_SHA1: map.get("FILE_SHA1").unwrap().to_owned(),
            FILE_SHA256: map.get("FILE_SHA256").unwrap().to_owned(),
            FILE_SHA384: map.get("FILE_SHA384").unwrap().to_owned(),
            FILE_SHA512: map.get("FILE_SHA512").unwrap().to_owned(),
            FILE_TIGER: map.get("FILE_TIGER").unwrap().to_owned(),
            FILE_WHIRLPOOL: map.get("FILE_WHIRLPOOL").unwrap().to_owned(),
            FILE_SSDEEP: map.get("FILE_SSDEEP").unwrap().to_owned(),
            FILE_TLSH: map.get("FILE_TLSH").unwrap().to_owned(),
            FILE_ATTRS: map.get("FILE_ATTRS").unwrap().to_owned(),

            // PE.to_owned(),
            PE_MD5: map.get("PE_MD5").unwrap().to_owned(),
            PE_SHA1: map.get("PE_SHA1").unwrap().to_owned(),
            PE_SHA256: map.get("PE_SHA256").unwrap().to_owned(),
            PE_ARCH: map.get("PE_ARCH").unwrap().to_owned(),
            PE_SUBSYSTEM: map.get("PE_SUBSYSTEM").unwrap().to_owned(),
            PE_VERSION: map.get("PE_VERSION").unwrap().to_owned(),
            EXIT_CODE: map.get("EXIT_CODE").unwrap().to_owned(),

            // File execution.to_owned(),
            COMMAND_LINE: map.get("COMMAND_LINE").unwrap().to_owned(),

            // Application.to_owned(),
            APP_ID: map.get("APP_ID").unwrap().to_owned(),
            APP_NAME: map.get("APP_NAME").unwrap().to_owned(),
            APP_CLSID: map.get("APP_CLSID").unwrap().to_owned(),
            MSI_PRODUCT: map.get("MSI_PRODUCT").unwrap().to_owned(),
            MSI_PACKAGE: map.get("MSI_PACKAGE").unwrap().to_owned(),

            // Publisher,
            COMPANY: map.get("COMPANY").unwrap().to_owned(),
            PUBLISHER: map.get("PUBLISHER").unwrap().to_owned(),
            PRODUCT: map.get("PRODUCT").unwrap().to_owned(),

            // Certificate,
            CERT_SHA1: map.get("CERT_SHA1").unwrap().to_owned(),

            // Registry,
            HIVE_MOUNT: map.get("HIVE_MOUNT").unwrap().to_owned(),
            KEY_NAME: map.get("KEY_NAME").unwrap().to_owned(),
            KEY_PATH: map.get("KEY_PATH").unwrap().to_owned(),
            VALUE_NAME: map.get("VALUE_NAME").unwrap().to_owned(),
            VALUE_DATA: map.get("VALUE_DATA").unwrap().to_owned(),

            // Service,
            SERVICE_NAME: map.get("SERVICE_NAME").unwrap().to_owned(),
            SERVICE_TYPE: map.get("SERVICE_TYPE").unwrap().to_owned(),
            SERVICE_DISPLAY_NAME: map.get("SERVICE_DISPLAY_NAME").unwrap().to_owned(),
            SERVICE_START_TYPE: map.get("SERVICE_START_TYPE").unwrap().to_owned(),

            // Process,
            PROCESS_ID: map.get("PROCESS_ID").unwrap().to_owned(),

            // ScheduledTask,
            SCHTASK_GUID: map.get("SCHTASK_GUID").unwrap().to_owned(),
            SCHTASK_URI: map.get("SCHTASK_URI").unwrap().to_owned(),

            // Event,
            EVT_PROVIDER: map.get("EVT_PROVIDER").unwrap().to_owned(),
            EVT_ID: map.get("EVT_ID").unwrap().to_owned(),
            EVT_CHANNEL: map.get("EVT_CHANNEL").unwrap().to_owned(),
            EVT_RECORD_ID: map.get("EVT_RECORD_ID").unwrap().to_owned(),

            // State,
            IN_USE: map.get("IN_USE").unwrap().to_owned(), // qualifies a boolean, used to tell deleted objects apart from existing ones
            REUSE_COUNT: map.get("REUSE_COUNT").unwrap().to_owned(), // qualifies an int, used to tell how many times an object has been reused

            // DNS,
            DOMAIN_NAME: map.get("DOMAIN_NAME").unwrap().to_owned(),

            // Windows,
            WINDOWS_PRIVILEGES: map.get("WINDOWS_PRIVILEGES").unwrap().to_owned(),
            SECURITY_DESCRIPTOR: map.get("SECURITY_DESCRIPTOR").unwrap().to_owned(),
            WINDOWS_OBJECT: map.get("WINDOWS_OBJECT").unwrap().to_owned(),

            // Network,
            IP_ADDRESS: map.get("IP_ADDRESS").unwrap().to_owned(),
            IP_PORT: map.get("IP_PORT").unwrap().to_owned(),
            MAC_ADDRESS: map.get("MAC_ADDRESS").unwrap().to_owned(),
        }
    }
}
impl Qualifiers {
    pub fn get(&self, name: &str) -> Result<&String, Error> {
        self.map
            .get(name)
            .ok_or(Error::UnknownQualifier(name.to_owned()))
    }
}
