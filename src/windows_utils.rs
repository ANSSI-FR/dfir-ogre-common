use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use pyo3::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use crate::{Error, Record, Value};

#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct SecurityDescriptor {
    control_flags: Vec<String>,
    owner_sid: String,
    group_sid: String,
    sacl_ace: Option<SecurityDescriptorAce>,
    dacl_ace: Option<SecurityDescriptorAce>,
}
#[pymethods]
impl SecurityDescriptor {
    pub fn to_record(&self) -> Record {
        let mut record = Record::new();
        record.add("owner_sid", Value::String(self.owner_sid.clone()));
        record.add("group_sid", Value::String(self.group_sid.clone()));
        let flags: Vec<Value> = self
            .control_flags
            .iter()
            .map(|flag| Value::String(flag.to_string()))
            .collect();
        record.add("control_flags", Value::Array(flags));

        if let Some(ace) = &self.sacl_ace {
            record.add("sacl_ace", Value::Object(ace.to_record()));
        }

        if let Some(ace) = &self.dacl_ace {
            record.add("dacl_ace", Value::Object(ace.to_record()));
        }

        record
    }
}

#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct SecurityDescriptorAce {
    ace_type: Option<String>,
    ace_flags: Vec<String>,
    rights: Option<Vec<String>>,
    account_sid: String,
}
impl SecurityDescriptorAce {
    pub fn to_record(&self) -> Record {
        let mut record = Record::new();
        if let Some(ace_type) = &self.ace_type {
            record.add("ace_type", Value::String(ace_type.clone()));
        }
        record.add("account_sid", Value::String(self.account_sid.clone()));
        let flags: Vec<Value> = self
            .ace_flags
            .iter()
            .map(|flag| Value::String(flag.to_string()))
            .collect();
        record.add("ace_flags", Value::Array(flags));

        if let Some(rights) = &self.rights {
            let rights: Vec<Value> = rights
                .iter()
                .map(|flag| Value::String(flag.to_string()))
                .collect();
            record.add("rights", Value::Array(rights));
        }

        record
    }
}

#[pyfunction]
pub fn security_descriptor_from_bytes(b: &[u8]) -> Result<SecurityDescriptor, Error> {
    let mut descriptor = SecurityDescriptor {
        ..Default::default()
    };

    if b.len() < 8 {
        return Ok(descriptor);
    }
    // # size_sd = b[0:4]
    // #every offset is relative to the start of the security descriptor which doesn't contain the size
    let sd_start = 4;
    // revision = b[4:6]
    let control_flags_bits = u16::from_le_bytes(
        b[6..8]
            .try_into()
            .map_err(|_| Error::InvalidByteLenght("control flags bits".into()))?,
    ) as u32;
    let mut control_flags = HashSet::new();

    for (key, value) in SD_BFLAGS.iter() {
        if key & control_flags_bits > 0 {
            control_flags.insert(value.to_owned());
        }
    }
    let flags: Vec<String> = control_flags.iter().map(|flag| flag.to_string()).collect();
    descriptor.control_flags = flags;

    if b.len() < 12 {
        return Ok(descriptor);
    }
    let owner_offset = u32::from_le_bytes(
        b[8..12]
            .try_into()
            .map_err(|_| Error::InvalidByteLenght("Owner Offset".into()))?,
    ) as usize;

    // sid format : Revision number (1) + Number of sub authorities (1) + Authority (6) + nb*sub authorities (nb*4)
    let start = sd_start + owner_offset + 1;

    if b.len() < start + 1 {
        return Ok(descriptor);
    }
    let owner_size = 8 + 4 * b[start] as usize;

    let start = sd_start + owner_offset;
    let stop = start + owner_size;
    if b.len() < stop {
        return Ok(descriptor);
    }
    let owner_sid = convert_sid(&b[start..stop])?;
    descriptor.owner_sid = owner_sid;

    // sid format : Revision number (1) + Number of sub authorities (1) + Authority (6) + nb*sub authorities (nb*4)

    // let arr: [u8; 4] = ;
    let group_offset = u32::from_le_bytes(
        b[12..16]
            .try_into()
            .map_err(|_| Error::InvalidByteLenght("group offset".into()))?,
    ) as usize;
    let start = sd_start + group_offset + 1;
    if b.len() < start + 1 {
        return Ok(descriptor);
    }
    let group_size = 8 + 4 * b[start] as usize;
    let start = sd_start + group_offset;
    let stop = start + group_size;
    if b.len() < stop {
        return Ok(descriptor);
    }
    let group_sid = convert_sid(&b[start..stop])?;
    descriptor.group_sid = group_sid;

    //sacl
    if control_flags.contains("SE_SACL_PRESENT") {
        let mut sacl_ace = vec![];
        if b.len() < 20 {
            return Ok(descriptor);
        }
        let sacl_offset = sd_start
            + u32::from_le_bytes(
                b[16..20]
                    .try_into()
                    .map_err(|_| Error::InvalidByteLenght("SACL offset".into()))?,
            ) as usize;

        let start = sacl_offset + 4;
        let stop = start + 2;
        if b.len() < stop {
            return Ok(descriptor);
        }
        let nb_ace = u16::from_le_bytes(
            b[start..stop]
                .try_into()
                .map_err(|_| Error::InvalidByteLenght("ACE number".into()))?,
        ) as usize;
        let mut c_offset = sacl_offset + 8;

        for _ in 0..nb_ace {
            let start = c_offset + 2;
            let stop = start + 2;
            if b.len() < stop {
                return Ok(descriptor);
            }
            let ace_size = u16::from_le_bytes(
                b[start..stop]
                    .try_into()
                    .map_err(|_| Error::InvalidByteLenght("ACE size".into()))?,
            ) as usize;

            let stop = c_offset + ace_size;
            if b.len() < stop {
                return Ok(descriptor);
            }
            let mut v = b[c_offset..stop].to_vec();
            sacl_ace.append(&mut v);

            c_offset += ace_size
        }
        let ace = ace_from_bytes(&sacl_ace)?;
        descriptor.sacl_ace = Some(ace);
    }
    //dacl
    if control_flags.contains("SE_DACL_PRESENT") {
        let mut dacl_ace = vec![];
        if b.len() < 24 {
            return Ok(descriptor);
        }

        let dacl_offset = sd_start
            + u32::from_le_bytes(
                b[20..24]
                    .try_into()
                    .map_err(|_| Error::InvalidByteLenght("Dacl Offset".into()))?,
            ) as usize;

        let start = dacl_offset + 4;
        let stop = start + 2;
        if b.len() < stop {
            return Ok(descriptor);
        }
        let nb_ace = u16::from_le_bytes(
            b[start..stop]
                .try_into()
                .map_err(|_| Error::InvalidByteLenght("ACE number".into()))?,
        ) as usize;

        let mut c_offset = dacl_offset + 8;
        for _ in 0..nb_ace {
            let start = c_offset + 2;
            let stop = start + 2;
            if b.len() < stop {
                return Ok(descriptor);
            }
            let ace_size = u16::from_le_bytes(
                b[start..stop]
                    .try_into()
                    .map_err(|_| Error::InvalidByteLenght("ACE".into()))?,
            ) as usize;
            let stop = c_offset + ace_size;
            if b.len() < stop {
                return Ok(descriptor);
            }
            let mut v = b[c_offset..stop].to_vec();
            dacl_ace.append(&mut v);
            c_offset += ace_size
        }
        let ace = ace_from_bytes(&dacl_ace)?;
        descriptor.dacl_ace = Some(ace);
    }

    Ok(descriptor)
}

fn ace_from_bytes(ace_b: &[u8]) -> Result<SecurityDescriptorAce, Error> {
    let mut sec_ace = SecurityDescriptorAce {
        ..Default::default()
    };
    if ace_b.len() < 2 {
        return Ok(sec_ace);
    }
    let ace_type_opt = ACE_BTYPE.get(&ace_b[0]);
    sec_ace.ace_type = ace_type_opt.map(|s| s.to_string());

    let ace_flags_b = ace_b[1];

    let mut ace_flags = HashSet::new();
    for (key, value) in ACE_BFLAGS.iter() {
        if key & ace_flags_b > 0 {
            ace_flags.insert(value.to_owned());
        }
    }

    let flags: Vec<String> = ace_flags.iter().map(|flag| flag.to_string()).collect();
    sec_ace.ace_flags = flags;

    if let Some(ace_type) = ace_type_opt
        && !ace_type.contains("OBJECT")
    {
        if ace_b.len() < 8 {
            return Ok(sec_ace);
        }
        let rights_b = u32::from_le_bytes(
            ace_b[4..8]
                .try_into()
                .map_err(|_| Error::InvalidByteLenght("ACE rights".into()))?,
        );

        let mut rights = vec![];
        for (key, value) in BRIGHTS.iter() {
            if key & rights_b > 0 {
                rights.push(value.to_string());
            }
        }
        sec_ace.rights = Some(rights);

        if ace_b.len() < 10 {
            return Ok(sec_ace);
        }
        let sid_size: usize = 8 + 4 * ace_b[8 + 1] as usize;
        let start = 8;
        let stop = start + sid_size;
        if ace_b.len() < stop {
            return Ok(sec_ace);
        }
        let account_sid = convert_sid(&ace_b[start..stop])?;

        sec_ace.account_sid = account_sid;
    } else {
        //let object_type = u32::from_le_bytes(ace_b[4..8].try_into().unwrap());
        // let mut index = 8;
        //ACE_OBJECT_TYPE_PRESENT
        // if object_type & 0x00000001 > 0 {
        //     //TODO: need test data to check the format
        //     // let object_guid = &ace_b[index..index + 8];
        //     //  self.object_guid = ace_b[index : index + 8]
        //     index += 8;
        // }
        //ACE_INHERITED_OBJECT_TYPE_PRESENT
        // if object_type & 0x00000002 > 0 {
        //     // self.inherit_object_guid = ace_b[index:index+8]
        //     index += 8;
        // }
        // #TODO: need test data to check the format
        // # self.inherit_object_guid = ace_b[index:index+8]

        // # sid format : Revision number (1) + Number of sub authorities (1) + Authority (6) + nb*sub authorities (nb*4)

        let start = 40;
        if ace_b.len() < start + 1 {
            return Ok(sec_ace);
        }
        let sid_size = 8 + 4 * ace_b[40] as usize;

        let stop = start + sid_size;
        if ace_b.len() < stop {
            return Ok(sec_ace);
        }
        let account_sid = convert_sid(&ace_b[start..stop])?;
        sec_ace.account_sid = account_sid;
    }

    Ok(sec_ace)
}

///  Converts a binary SID to its string representation.
/// https://msdn.microsoft.com/en-us/library/windows/desktop/aa379597.aspx
/// The byte representation of an SID is as follows:
/// Offset  Length  Description
/// 00      01      revision
/// 01      01      sub-authority count
/// 02      06      authority (big endian)
/// 08      04      subauthority #1 (little endian)
/// 0b      04      subauthority #2 (little endian)
/// # Arguments
/// * `b_sid` - The binary SID data.
pub fn convert_sid(b_sid: &[u8]) -> Result<String, Error> {
    // sid[0] is the Revision, we allow only version 1, because it's the
    // only version that exists right now.
    let mut sid = "S-1-".to_string();

    // The next byte specifies the numbers of sub authorities
    // (number of dashes minus two), should be 5 or less, but not enforcing that
    if b_sid.len() < 2 {
        return Ok(sid);
    }
    let sub_authority_count = b_sid[1];

    // identifier authority (6 bytes starting from the second) (big endian)
    let mut identifier_authority: u64 = 0;
    let offset = 2;
    let size = 6;
    for i in 0..size {
        if b_sid.len() < offset + i + 1 {
            return Ok(sid);
        }
        identifier_authority |= (b_sid[offset + i] as u64) << (8 * (size - 1 - i));
    }
    sid.push_str(&identifier_authority.to_string());

    // Iterate all the sub authorities (little-endian)
    let mut offset = 8;
    let size = 4; // 32-bits (4 bytes) for sub authorities

    for _ in 0..sub_authority_count {
        if b_sid.len() < offset + size {
            return Ok(sid);
        }
        let arr: [u8; 4] = b_sid[offset..offset + size].try_into().unwrap();
        let sub_authority = u32::from_le_bytes(arr);
        sid.push('-');
        sid.push_str(&sub_authority.to_string());
        offset += size;
    }
    Ok(sid)
}

//
// parse TimeStamp in the FILETIME format: the number of 100-nanosecond intervals since January 1, 1601 (UTC).
//
pub fn from_filetime(timestamp: u64) -> DateTime<Utc> {
    let naive = NaiveDate::from_ymd_opt(1601, 1, 1)
        .and_then(|x| x.and_hms_nano_opt(0, 0, 0, 0))
        .expect("to_datetime() should work")
        + Duration::microseconds((timestamp / 10) as i64);

    let result = Utc.from_local_datetime(&naive);
    result.earliest().unwrap_or(Utc::now())
}

static _ACE_TYPE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, &'static str> = HashMap::new();
    m.insert("A", "ACCESS_ALLOWED_ACE_TYPE");
    m.insert("D", "ACCESS_DENIED_ACE_TYPE");
    m.insert("OA", "ACCESS_ALLOWED_OBJECT_ACE_TYPE");
    m.insert("OD", "ACCESS_DENIED_OBJECT_ACE_TYPE");
    m.insert("AU", "SYSTEM_AUDIT_ACE_TYPE");
    m.insert("AL", "SYSTEM_ALARM_ACE_TYPE");
    m.insert("OU", "SYSTEM_AUDIT_OBJECT_ACE_TYPE");
    m.insert("OL", "SYSTEM_ALARM_OBJECT_ACE_TYPE");
    m.insert("ML", "SYSTEM_MANDATORY_LABEL_ACE_TYPE");
    m.insert("XA", "ACCESS_ALLOWED_CALLBACK_ACE_TYPE");
    m.insert("XD", "ACCESS_DENIED_CALLBACK_ACE_TYPE");
    m.insert("RA", "SYSTEM_RESOURCE_ATTRIBUTE_ACE_TYPE");
    m.insert("SP", "SYSTEM_SCOPED_POLICY_ID_ACE_TYPE");
    m.insert("XU", "SYSTEM_AUDIT_CALLBACK_ACE_TYPE");
    m.insert("ZA", "ACCESS_ALLOWED_CALLBACK_ACE_TYPE");
    m.insert("TL", "SYSTEM_PROCESS_TRUST_LABEL_ACE_TYPE");
    m.insert("FL", "SYSTEM_ACCESS_FILTER_ACE_TYPE");

    m
});

static ACE_BTYPE: LazyLock<HashMap<u8, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<u8, &'static str> = HashMap::new();
    m.insert(0x00, "ACCESS_ALLOWED_ACE_TYPE");
    m.insert(0x01, "ACCESS_DENIED_ACE_TYPE");
    m.insert(0x02, "SYSTEM_AUDIT_ACE_TYPE");
    m.insert(0x03, "SYSTEM_ALARM_ACE_TYPE");
    m.insert(0x04, "ACCESS_ALLOWED_COMPOUND_ACE_TYPE");
    m.insert(0x05, "ACCESS_ALLOWED_OBJECT_ACE_TYPE");
    m.insert(0x06, "ACCESS_DENIED_OBJECT_ACE_TYPE");
    m.insert(0x07, "SYSTEM_AUDIT_OBJECT_ACE_TYPE");
    m.insert(0x08, "SYSTEM_ALARM_OBJECT_ACE_TYPE");
    m.insert(0x09, "ACCESS_ALLOWED_CALLBACK_ACE_TYPE");
    m.insert(0x0a, "ACCESS_DENIED_CALLBACK_ACE_TYPE");
    m.insert(0x0b, "ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE");
    m.insert(0x0c, "ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE");
    m.insert(0x0d, "SYSTEM_AUDIT_CALLBACK_ACE_TYPE");
    m.insert(0x0e, "SYSTEM_ALARM_CALLBACK_ACE_TYPE");
    m.insert(0x0f, "SYSTEM_AUDIT_CALLBACK_OBJECT_ACE_TYPE");
    m.insert(0x10, "SYSTEM_ALARM_CALLBACK_OBJECT_ACE_TYPE");
    m.insert(0x11, "SYSTEM_MANDATORY_LABEL_ACE_TYPE");
    m
});

static _ACE_FLAGS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, &'static str> = HashMap::new();
    m.insert("CI", "CONTAINER_INHERIT_ACE"); // CONTAINER_INHERIT_ACE
    m.insert("OI", "OBJECT_INHERIT_ACE"); // OBJECT_INHERIT_ACE
    m.insert("NP", "NO_PROPAGATE_INHERIT_ACE"); // NO_PROPAGATE_INHERIT_ACE
    m.insert("IO", "INHERIT_ONLY_ACE"); //
    m.insert("ID", "INHERITED_ACE"); // INHERITED_ACE
    m.insert("SA", "SUCCESSFUL_ACCESS_ACE_FLAG"); // SUCCESSFUL_ACCESS_ACE_FLAG
    m.insert("FA", "FAILED_ACCESS_ACE_FLAG"); // FAILED_ACCESS_ACE_FLAG
    m.insert("TP", "TRUST_PROTECTED_FILTER_ACE_FLAG"); // TRUST_PROTECTED_FILTER_ACE_FLAG
    m.insert("CR", "CRITICAL_ACE_FLAG"); // CRITICAL_ACE_FLAG
    m
});

static ACE_BFLAGS: LazyLock<HashMap<u8, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<u8, &'static str> = HashMap::new();
    m.insert(0x01, "OBJECT_INHERIT_ACE"); // OBJECT_INHERIT_ACE
    m.insert(0x02, "CONTAINER_INHERIT_ACE"); // CONTAINER_INHERIT_ACE
    m.insert(0x04, "NO_PROPAGATE_INHERIT_ACE"); // NO_PROPAGATE_INHERIT_ACE
    m.insert(0x08, "INHERIT_ONLY_ACE"); // INHERIT_ONLY_ACE
    m.insert(0x40, "SUCCESSFUL_ACCESS_ACE_FLAG"); // SUCCESSFUL_ACCESS_ACE_FLAG
    m.insert(0x80, "FAILED_ACCESS_ACE_FLAG"); // FAILED_ACCESS_ACE_FLAG
    m
});

static _RIGHTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, &'static str> = HashMap::new();
    m.insert("GA", "GENERIC_ALL");
    m.insert("GR", "GENERIC_READ");
    m.insert("GW", "GENERIC_WRITE");
    m.insert("GX", "GENERIC_EXECUTE");
    m.insert("RC", "READ_CONTROL");
    m.insert("SD", "DELETE");
    m.insert("WD", "WRITE_DAC");
    m.insert("WO", "WRITE_OWNER");
    m.insert("RP", "ADS_RIGHT_DS_READ_PROP");
    m.insert("WP", "ADS_RIGHT_DS_WRITE_PROP");
    m.insert("CC", "ADS_RIGHT_DS_CREATE_CHILD");
    m.insert("DC", "ADS_RIGHT_DS_DELETE_CHILD");
    m.insert("LC", "ADS_RIGHT_ACTRL_DS_LIST");
    m.insert("SW", "ADS_RIGHT_DS_SELF");
    m.insert("LO", "ADS_RIGHT_DS_LIST_OBJECT");
    m.insert("DT", "ADS_RIGHT_DS_DELETE_TREE");
    m.insert("CR", "ADS_RIGHT_DS_CONTROL_ACCESS");
    m.insert("FA", "FILE_GENERIC_ALL");
    m.insert("FR", "FILE_GENERIC_READ");
    m.insert("FW", "FILE_GENERIC_WRITE");
    m.insert("FX", "FILE_GENERIC_EXECUTE");
    m.insert("KA", "KEY_ALL_ACCESS");
    m.insert("KR", "KEY_READ");
    m.insert("KW", "KEY_WRITE");
    m.insert("KX", "KEY_EXECUTE");
    m.insert("NR", "SYSTEM_MANDATORY_LABEL_NO_READ_UP");
    m.insert("NW", "SYSTEM_MANDATORY_LABEL_NO_WRITE_UP");
    m.insert("NX", "SYSTEM_MANDATORY_LABEL_NO_EXECUTE_UP");
    m
});

static BRIGHTS: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<u32, &'static str> = HashMap::new();
    m.insert(0x000F003F, "KEY_ALL_ACCESS");
    m.insert(0x00000020, "KEY_CREATE_LINK");
    m.insert(0x00000004, "KEY_CREATE_SUB_KEY");
    m.insert(0x00000008, "KEY_ENUMERATE_SUB_KEYS");
    m.insert(0x00000010, "KEY_NOTIFY");
    m.insert(0x00000001, "KEY_QUERY_VALUE");
    m.insert(0x00020019, "KEY_READ");
    m.insert(0x00000002, "KEY_SET_VALUE");
    m.insert(0x00000200, "KEY_WOW64_32KEY");
    m.insert(0x00000100, "KEY_WOW64_64KEY");
    m.insert(0x00020006, "KEY_WRITE");
    m.insert(0x01000000, "ACCESS_SYSTEM_SECURITY");
    m.insert(0x02000000, "MAXIMUM_ALLOWED");
    m.insert(0x10000000, "GENERIC_ALL");
    m.insert(0x20000000, "GENERIC_EXECUTE");
    m.insert(0x40000000, "GENERIC_WRITE");
    m.insert(0x80000000, "GENERIC_READ");
    m
});

static _SID_STRING: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, &'static str> = HashMap::new();
    m.insert("AA", "DOMAIN_ALIAS_RID_ACCESS_CONTROL_ASSISTANCE_OPS");
    m.insert("AC", "SECURITY_BUILTIN_PACKAGE_ANY_PACKAGE");
    m.insert("AN", "SECURITY_ANONYMOUS_LOGON_RID");
    m.insert("AO", "DOMAIN_ALIAS_RID_ACCOUNT_OPS");
    m.insert("AP", "DOMAIN_GROUP_RID_PROTECTED_USERS");
    m.insert("AU", "SECURITY_AUTHENTICATED_USER_RID");
    m.insert("BA", "DOMAIN_ALIAS_RID_ADMINS");
    m.insert("BG", "DOMAIN_ALIAS_RID_GUESTS");
    m.insert("BO", "DOMAIN_ALIAS_RID_BACKUP_OPS");
    m.insert("BU", "DOMAIN_ALIAS_RID_USERS");
    m.insert("CA", "DOMAIN_GROUP_RID_CERT_ADMINS");
    m.insert("CD", "DOMAIN_ALIAS_RID_CERTSVC_DCOM_ACCESS_GROUP");
    m.insert("CG", "SECURITY_CREATOR_GROUP_RID");
    m.insert("CN", "DOMAIN_GROUP_RID_CLONEABLE_CONTROLLERS");
    m.insert("CO", "SECURITY_CREATOR_OWNER_RID");
    m.insert("CY", "DOMAIN_ALIAS_RID_CRYPTO_OPERATORS");
    m.insert("DA", "DOMAIN_GROUP_RID_ADMINS");
    m.insert("DC", "DOMAIN_GROUP_RID_COMPUTERS");
    m.insert("DD", "DOMAIN_GROUP_RID_CONTROLLERS");
    m.insert("DG", "DOMAIN_GROUP_RID_GUESTS");
    m.insert("DU", "DOMAIN_GROUP_RID_USERS");
    m.insert("EA", "DOMAIN_GROUP_RID_ENTERPRISE_ADMINS");
    m.insert("ED", "SECURITY_SERVER_LOGON_RID");
    m.insert("EK", "DOMAIN_GROUP_RID_ENTERPRISE_KEY_ADMINS");
    m.insert("ER", "DOMAIN_ALIAS_RID_EVENT_LOG_READERS_GROUP");
    m.insert("ES", "DOMAIN_ALIAS_RID_RDS_ENDPOINT_SERVERS");
    m.insert("HA", "DOMAIN_ALIAS_RID_HYPER_V_ADMINS");
    m.insert("HI", "SECURITY_MANDATORY_HIGH_RID");
    m.insert("IS", "DOMAIN_ALIAS_RID_IUSERS");
    m.insert("IU", "SECURITY_INTERACTIVE_RID");
    m.insert("KA", "DOMAIN_GROUP_RID_KEY_ADMINS");
    m.insert("LA", "DOMAIN_USER_RID_ADMIN");
    m.insert("LG", "DOMAIN_USER_RID_GUEST");
    m.insert("LS", "SECURITY_LOCAL_SERVICE_RID");
    m.insert("LU", "DOMAIN_ALIAS_RID_LOGGING_USERS");
    m.insert("LW", "SECURITY_MANDATORY_LOW_RID");
    m.insert("ME", "SECURITY_MANDATORY_MEDIUM_RID");
    m.insert("MP", "SECURITY_MANDATORY_MEDIUM_PLUS_RID");
    m.insert("MU", "DOMAIN_ALIAS_RID_MONITORING_USERS");
    m.insert("NO", "DOMAIN_ALIAS_RID_NETWORK_CONFIGURATION_OPS");
    m.insert("NS", "SECURITY_NETWORK_SERVICE_RID");
    m.insert("NU", "SECURITY_NETWORK_RID");
    m.insert("OW", "SECURITY_CREATOR_OWNER_RIGHTS_RID");
    m.insert("PA", "DOMAIN_GROUP_RID_POLICY_ADMINS");
    m.insert("PO", "DOMAIN_ALIAS_RID_PRINT_OPS");
    m.insert("PS", "SECURITY_PRINCIPAL_SELF_RID");
    m.insert("PU", "DOMAIN_ALIAS_RID_POWER_USERS");
    m.insert("RA", "DOMAIN_ALIAS_RID_RDS_REMOTE_ACCESS_SERVERS");
    m.insert("RC", "SECURITY_RESTRICTED_CODE_RID");
    m.insert("RD", "DOMAIN_ALIAS_RID_REMOTE_DESKTOP_USERS");
    m.insert("RE", "DOMAIN_ALIAS_RID_REPLICATOR");
    m.insert("RM", "SDDL_RMS__SERVICE_OPERATORS");
    m.insert(
        "RO",
        "DOMAIN_GROUP_RID_ENTERPRISE_READONLY_DOMAIN_CONTROLLERS",
    );
    m.insert("RS", "DOMAIN_ALIAS_RID_RAS_SERVERS");
    m.insert("RU", "DOMAIN_ALIAS_RID_PREW2KCOMPACCESS");
    m.insert("SA", "DOMAIN_GROUP_RID_SCHEMA_ADMINS");
    m.insert("SI", "SECURITY_MANDATORY_SYSTEM_RID");
    m.insert("SO", "DOMAIN_ALIAS_RID_SYSTEM_OPS");
    m.insert("SS", "SECURITY_AUTHENTICATION_SERVICE_ASSERTED_RID");
    m.insert("SU", "SECURITY_SERVICE_RID");
    m.insert("SY", "SECURITY_LOCAL_SYSTEM_RID");
    m.insert("UD", "SECURITY_USERMODEDRIVERHOST_ID_BASE_RID");
    m.insert("WD", "SECURITY_WORLD_RID");
    m.insert("WR", "SECURITY_WRITE_RESTRICTED_CODE_RID");
    m
});

static _DACL_FLAGS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, &'static str> = HashMap::new();
    m.insert("P", "SDDL_PROTECTED");
    m.insert("AR", "SDDL_AUTO_INHERIT_REQ");
    m.insert("AI", "SDDL_AUTO_INHERITED");
    m.insert("NO_ACCESS_CONTROL", "SDDL_NULL_ACL");
    m
});

static SD_BFLAGS: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    let mut m: HashMap<u32, &'static str> = HashMap::new();
    m.insert(0x0001, "SE_OWNER_DEFAULTED");
    m.insert(0x0002, "SE_GROUP_DEFAULTED");
    m.insert(0x0004, "SE_DACL_PRESENT");
    m.insert(0x0008, "SE_DACL_DEFAULTED");
    m.insert(0x0010, "SE_SACL_PRESENT");
    m.insert(0x0020, "SE_SACL_DEFAULTED");
    m.insert(0x0100, "SE_DACL_AUTO_INHERIT_REQ");
    m.insert(0x0200, "SE_SACL_AUTO_INHERIT_REQ");
    m.insert(0x0400, "SE_DACL_AUTO_INHERITED");
    m.insert(0x0800, "SE_SACL_AUTO_INHERITED");
    m.insert(0x1000, "SE_DACL_PROTECTED");
    m.insert(0x2000, "SE_SACL_PROTECTED");
    m.insert(0x4000, "SE_RM_CONTROL_VALID");
    m.insert(0x8000, "SE_SELF_RELATIVE");
    m
});

#[cfg(test)]
mod tests {

    use rand::random;

    use super::*;

    #[test]
    fn descriptor() {
        let security_descriptor = "0801000001000488dc000000ec00000000000000140000000200c80007000000000218001900020001020000000000052000000021020000000218003f000f0001020000000000052000000020020000000214003f000f00010100000000000512000000000018003f000f0001020000000000052000000020020000001a14003f000f000101000000000003000000000002180019000200010200000000000f02000000010000000002380019000200010a00000000000f0300000000040000b031803f6cbc634c3ce050d1970ca1620f01cb197e7aa6c0fae697f119a30cce010200000000000520000000200200000105000000000005150000006441f6999e6ec4c34d2890de01020000";

        let sec_descp = hex::decode(security_descriptor).unwrap();
        let res = security_descriptor_from_bytes(&sec_descp).unwrap();

        assert_eq!(res.owner_sid, "S-1-5-32-544");
        assert_eq!(
            res.group_sid,
            "S-1-5-21-2583052644-3284430494-3733989453-513"
        );

        let dacl_ace = &res.dacl_ace.unwrap();

        assert_eq!(dacl_ace.account_sid, "S-1-5-32-545");
        assert_eq!(
            dacl_ace.ace_type.as_ref().unwrap(),
            "ACCESS_ALLOWED_ACE_TYPE"
        );
    }

    #[test]
    fn descriptor_fuzzing() {
        for _ in 0..10_000 {
            let rng_size: u8 = random();

            let mut data: Vec<u8> = vec![0; rng_size as usize];

            rand::fill(&mut data[..]);
            security_descriptor_from_bytes(&data).unwrap();
        }
    }

    #[test]
    fn ace_fuzzing() {
        for _ in 0..10_000 {
            let rng_size: u8 = random();

            let mut data: Vec<u8> = vec![0; rng_size as usize];

            rand::fill(&mut data[..]);
            ace_from_bytes(&data).unwrap();
        }
    }

    #[test]
    fn convert_sid_fuzzing() {
        for _ in 0..10_000 {
            let rng_size: u8 = random();

            let mut data: Vec<u8> = vec![0; rng_size as usize];

            rand::fill(&mut data[..]);
            convert_sid(&data).unwrap();
        }
    }
}
