use chrono::{DateTime, Utc};
use pyo3::prelude::*;
use std::{collections::HashMap, sync::LazyLock};

use crate::{Error, Record, Value, date_util::filetime_to_datetime};

const SD_START: usize = 4;

const SD_BFLAGS_ORDERED: &[(u16, &str)] = &[
    (0x0001, "SE_OWNER_DEFAULTED"),
    (0x0002, "SE_GROUP_DEFAULTED"),
    (0x0004, "SE_DACL_PRESENT"),
    (0x0008, "SE_DACL_DEFAULTED"),
    (0x0010, "SE_SACL_PRESENT"),
    (0x0020, "SE_SACL_DEFAULTED"),
    (0x0100, "SE_DACL_AUTO_INHERIT_REQ"),
    (0x0200, "SE_SACL_AUTO_INHERIT_REQ"),
    (0x0400, "SE_DACL_AUTO_INHERITED"),
    (0x0800, "SE_SACL_AUTO_INHERITED"),
    (0x1000, "SE_DACL_PROTECTED"),
    (0x2000, "SE_SACL_PROTECTED"),
    (0x4000, "SE_RM_CONTROL_VALID"),
    (0x8000, "SE_SELF_RELATIVE"),
];

const ACE_BFLAGS_ORDERED: &[(u8, &str)] = &[
    (0x01, "OBJECT_INHERIT_ACE"),
    (0x02, "CONTAINER_INHERIT_ACE"),
    (0x04, "NO_PROPAGATE_INHERIT_ACE"),
    (0x08, "INHERIT_ONLY_ACE"),
    (0x40, "SUCCESSFUL_ACCESS_ACE_FLAG"),
    (0x80, "FAILED_ACCESS_ACE_FLAG"),
];

const BRIGHTS_ORDERED: &[(u32, &str)] = &[
    (0x000F_003F, "KEY_ALL_ACCESS"),
    (0x0000_0020, "KEY_CREATE_LINK"),
    (0x0000_0004, "KEY_CREATE_SUB_KEY"),
    (0x0000_0008, "KEY_ENUMERATE_SUB_KEYS"),
    (0x0000_0010, "KEY_NOTIFY"),
    (0x0000_0001, "KEY_QUERY_VALUE"),
    (0x0002_0019, "KEY_READ"),
    (0x0000_0002, "KEY_SET_VALUE"),
    (0x0000_0200, "KEY_WOW64_32KEY"),
    (0x0000_0100, "KEY_WOW64_64KEY"),
    (0x0002_0006, "KEY_WRITE"),
    (0x0100_0000, "ACCESS_SYSTEM_SECURITY"),
    (0x0200_0000, "MAXIMUM_ALLOWED"),
    (0x1000_0000, "GENERIC_ALL"),
    (0x2000_0000, "GENERIC_EXECUTE"),
    (0x4000_0000, "GENERIC_WRITE"),
    (0x8000_0000, "GENERIC_READ"),
];

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(slice.try_into().ok()?))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn slice_at(bytes: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(len)?)
}

fn decode_u16_flags(bits: u16, table: &[(u16, &str)]) -> Vec<String> {
    table
        .iter()
        .filter_map(|(mask, value)| (bits & mask != 0).then(|| (*value).to_string()))
        .collect()
}

fn decode_u8_flags(bits: u8, table: &[(u8, &str)]) -> Vec<String> {
    table
        .iter()
        .filter_map(|(mask, value)| (bits & mask != 0).then(|| (*value).to_string()))
        .collect()
}

fn decode_u32_flags(bits: u32, table: &[(u32, &str)]) -> Vec<String> {
    table
        .iter()
        .filter_map(|(mask, value)| (bits & mask != 0).then(|| (*value).to_string()))
        .collect()
}

fn sid_at(bytes: &[u8], offset: usize) -> Option<String> {
    let sub_authority_count = *bytes.get(offset.checked_add(1)?)? as usize;
    let sid_size = 8usize.checked_add(4usize.checked_mul(sub_authority_count)?)?;
    let sid_bytes = slice_at(bytes, offset, sid_size)?;
    convert_sid(sid_bytes).ok()
}

fn has_flag(flags: &[String], needle: &str) -> bool {
    flags.iter().any(|flag| flag == needle)
}

#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct SecurityDescriptor {
    control_flags: Vec<String>,
    owner_sid: String,
    group_sid: String,
    sacl_aces: Vec<SecurityDescriptorAce>,
    dacl_aces: Vec<SecurityDescriptorAce>,
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

        let sacl_aces = self
            .sacl_aces
            .iter()
            .map(|ace| Value::Object(ace.to_record()))
            .collect();
        record.add("sacl_aces", Value::Array(sacl_aces));

        let dacl_aces = self
            .dacl_aces
            .iter()
            .map(|ace| Value::Object(ace.to_record()))
            .collect();
        record.add("dacl_aces", Value::Array(dacl_aces));

        record
    }
}

#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct SecurityDescriptorAce {
    ace_type: Option<String>,
    ace_flags: Vec<String>,
    rights: Option<Vec<String>>,
    account_sid: Option<String>,
    ace_size: Option<u16>,
    object_type_guid: Option<String>,
    inherited_object_type_guid: Option<String>,
    raw_hex: Option<String>,
}
impl SecurityDescriptorAce {
    pub fn to_record(&self) -> Record {
        let mut record = Record::new();
        if let Some(ace_type) = &self.ace_type {
            record.add("ace_type", Value::String(ace_type.clone()));
        }
        if let Some(account_sid) = &self.account_sid {
            record.add("account_sid", Value::String(account_sid.clone()));
        }
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
        if let Some(ace_size) = self.ace_size {
            record.add("ace_size", Value::Int(i64::from(ace_size)));
        }
        if let Some(object_type_guid) = &self.object_type_guid {
            record.add("object_type_guid", Value::String(object_type_guid.clone()));
        }
        if let Some(inherited_object_type_guid) = &self.inherited_object_type_guid {
            record.add(
                "inherited_object_type_guid",
                Value::String(inherited_object_type_guid.clone()),
            );
        }
        if let Some(raw_hex) = &self.raw_hex {
            record.add("raw_hex", Value::String(raw_hex.clone()));
        }

        record
    }
}

fn ace_type_name(ace_type: u8) -> String {
    ACE_BTYPE.get(&ace_type).map_or_else(
        || format!("UNKNOWN_ACE_TYPE_0x{ace_type:02X}"),
        |value| (*value).to_string(),
    )
}

fn is_known_ace_type(ace_type: u8) -> bool {
    ACE_BTYPE.contains_key(&ace_type)
}

fn is_object_ace_type(ace_type: u8) -> bool {
    matches!(
        ace_type,
        0x05 | 0x06 | 0x07 | 0x08 | 0x0b | 0x0c | 0x0f | 0x10
    )
}

fn format_guid_le(bytes: &[u8]) -> Option<String> {
    let guid = slice_at(bytes, 0, 16)?;
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid[3],
        guid[2],
        guid[1],
        guid[0],
        guid[5],
        guid[4],
        guid[7],
        guid[6],
        guid[8],
        guid[9],
        guid[10],
        guid[11],
        guid[12],
        guid[13],
        guid[14],
        guid[15]
    ))
}

fn acl_at(bytes: &[u8], offset: usize) -> Result<Vec<SecurityDescriptorAce>, Error> {
    let Some(ace_count) = read_u16_le(bytes, offset + 4) else {
        return Ok(vec![]);
    };

    let mut aces = Vec::with_capacity(ace_count as usize);
    let mut current = offset + 8;

    for _ in 0..ace_count {
        let Some(ace_type) = bytes.get(current).copied() else {
            break;
        };
        let Some(ace_size) = read_u16_le(bytes, current + 2) else {
            let partial = &bytes[current..];
            let mut ace = ace_from_bytes(partial)?;
            ace.ace_type = Some(ace_type_name(ace_type));
            ace.raw_hex = Some(hex::encode(partial));
            aces.push(ace);
            break;
        };

        if ace_size < 4 {
            let partial =
                slice_at(bytes, current, bytes.len().saturating_sub(current)).unwrap_or_default();
            let mut ace = ace_from_bytes(partial)?;
            ace.ace_size = Some(ace_size);
            ace.raw_hex = Some(hex::encode(partial));
            aces.push(ace);
            break;
        }

        let available = bytes.len().saturating_sub(current);
        let take = available.min(ace_size as usize);
        let Some(ace_bytes) = slice_at(bytes, current, take) else {
            break;
        };
        let mut ace = ace_from_bytes(ace_bytes)?;
        ace.ace_size = Some(ace_size);
        if take < ace_size as usize {
            ace.raw_hex = Some(hex::encode(ace_bytes));
            aces.push(ace);
            break;
        }

        aces.push(ace);
        current += ace_size as usize;
    }

    Ok(aces)
}

fn descriptor_relative_offset(bytes: &[u8], offset: usize) -> Option<usize> {
    let relative = read_u32_le(bytes, offset)? as usize;
    SD_START.checked_add(relative)
}

#[pyfunction]
pub fn security_descriptor_from_bytes(b: &[u8]) -> Result<SecurityDescriptor, Error> {
    let mut descriptor = SecurityDescriptor {
        ..Default::default()
    };

    let Some(control_flags_bits) = read_u16_le(b, SD_START + 2) else {
        return Ok(descriptor);
    };
    descriptor.control_flags = decode_u16_flags(control_flags_bits, SD_BFLAGS_ORDERED);

    if let Some(owner_offset) = read_u32_le(b, SD_START + 4)
        && let Some(owner_start) = SD_START.checked_add(owner_offset as usize)
        && let Some(owner_sid) = sid_at(b, owner_start)
    {
        descriptor.owner_sid = owner_sid;
    }

    if let Some(group_offset) = read_u32_le(b, SD_START + 8)
        && let Some(group_start) = SD_START.checked_add(group_offset as usize)
        && let Some(group_sid) = sid_at(b, group_start)
    {
        descriptor.group_sid = group_sid;
    }

    if has_flag(&descriptor.control_flags, "SE_SACL_PRESENT")
        && let Some(sacl_offset) = descriptor_relative_offset(b, SD_START + 12)
    {
        descriptor.sacl_aces = acl_at(b, sacl_offset)?;
    }

    if has_flag(&descriptor.control_flags, "SE_DACL_PRESENT")
        && let Some(dacl_offset) = descriptor_relative_offset(b, SD_START + 16)
    {
        descriptor.dacl_aces = acl_at(b, dacl_offset)?;
    }

    Ok(descriptor)
}

fn ace_from_bytes(ace_b: &[u8]) -> Result<SecurityDescriptorAce, Error> {
    ace_from_bytes_with_declared_size(ace_b, read_u16_le(ace_b, 2))
}

fn ace_from_bytes_with_declared_size(
    ace_b: &[u8],
    declared_size: Option<u16>,
) -> Result<SecurityDescriptorAce, Error> {
    let mut sec_ace = SecurityDescriptorAce {
        ace_size: declared_size,
        ..Default::default()
    };

    let Some(ace_type_byte) = ace_b.first().copied() else {
        return Ok(sec_ace);
    };

    sec_ace.ace_type = Some(ace_type_name(ace_type_byte));
    if let Some(ace_flags_b) = ace_b.get(1).copied() {
        sec_ace.ace_flags = decode_u8_flags(ace_flags_b, ACE_BFLAGS_ORDERED);
    }

    if !is_known_ace_type(ace_type_byte) {
        sec_ace.raw_hex = Some(hex::encode(ace_b));
        return Ok(sec_ace);
    }

    let Some(rights_b) = read_u32_le(ace_b, 4) else {
        if !ace_b.is_empty() {
            sec_ace.raw_hex = Some(hex::encode(ace_b));
        }
        return Ok(sec_ace);
    };
    sec_ace.rights = Some(decode_u32_flags(rights_b, BRIGHTS_ORDERED));

    let sid_start = if is_object_ace_type(ace_type_byte) {
        let Some(object_flags) = read_u32_le(ace_b, 8) else {
            sec_ace.raw_hex = Some(hex::encode(ace_b));
            return Ok(sec_ace);
        };

        let mut offset = 12usize;
        if object_flags & 0x0000_0001 != 0 {
            let Some(guid_bytes) = slice_at(ace_b, offset, 16) else {
                sec_ace.raw_hex = Some(hex::encode(ace_b));
                return Ok(sec_ace);
            };
            sec_ace.object_type_guid = format_guid_le(guid_bytes);
            offset += 16;
        }

        if object_flags & 0x0000_0002 != 0 {
            let Some(guid_bytes) = slice_at(ace_b, offset, 16) else {
                sec_ace.raw_hex = Some(hex::encode(ace_b));
                return Ok(sec_ace);
            };
            sec_ace.inherited_object_type_guid = format_guid_le(guid_bytes);
            offset += 16;
        }
        offset
    } else {
        8
    };

    if let Some(account_sid) = sid_at(ace_b, sid_start) {
        sec_ace.account_sid = Some(account_sid);
    } else {
        sec_ace.raw_hex = Some(hex::encode(ace_b));
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
pub fn from_filetime(timestamp: u64) -> Result<DateTime<Utc>, Error> {
    filetime_to_datetime(timestamp)
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

#[cfg(test)]
mod tests {

    use super::*;

    fn sid_builtin_admins() -> Vec<u8> {
        vec![
            0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x20, 0x02,
            0x00, 0x00,
        ]
    }

    fn sid_builtin_users() -> Vec<u8> {
        vec![
            0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x21, 0x02,
            0x00, 0x00,
        ]
    }

    fn sid_everyone() -> Vec<u8> {
        vec![
            0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        ]
    }

    fn access_ace(ace_type: u8, ace_flags: u8, rights: u32, sid: &[u8]) -> Vec<u8> {
        let ace_size = 8 + sid.len();
        let mut ace = vec![ace_type, ace_flags];
        ace.extend_from_slice(&(ace_size as u16).to_le_bytes());
        ace.extend_from_slice(&rights.to_le_bytes());
        ace.extend_from_slice(sid);
        ace
    }

    fn object_ace(
        ace_type: u8,
        ace_flags: u8,
        rights: u32,
        object_flags: u32,
        object_guid: Option<[u8; 16]>,
        inherited_guid: Option<[u8; 16]>,
        sid: &[u8],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&rights.to_le_bytes());
        body.extend_from_slice(&object_flags.to_le_bytes());
        if let Some(guid) = object_guid {
            body.extend_from_slice(&guid);
        }
        if let Some(guid) = inherited_guid {
            body.extend_from_slice(&guid);
        }
        body.extend_from_slice(sid);

        let ace_size = 4 + body.len();
        let mut ace = vec![ace_type, ace_flags];
        ace.extend_from_slice(&(ace_size as u16).to_le_bytes());
        ace.extend_from_slice(&body);
        ace
    }

    fn acl_with_aces(aces: &[Vec<u8>]) -> Vec<u8> {
        let acl_size = 8 + aces.iter().map(Vec::len).sum::<usize>();
        let mut acl = vec![0x02, 0x00];
        acl.extend_from_slice(&(acl_size as u16).to_le_bytes());
        acl.extend_from_slice(&(aces.len() as u16).to_le_bytes());
        acl.extend_from_slice(&0u16.to_le_bytes());
        for ace in aces {
            acl.extend_from_slice(ace);
        }
        acl
    }

    fn descriptor_with_dacl(aces: &[Vec<u8>]) -> Vec<u8> {
        descriptor_with_raw_dacl(acl_with_aces(aces))
    }

    fn descriptor_with_raw_dacl(dacl: Vec<u8>) -> Vec<u8> {
        let owner = sid_builtin_admins();
        let group = sid_everyone();
        let owner_offset = 20u32;
        let group_offset = owner_offset + owner.len() as u32;
        let dacl_offset = group_offset + group.len() as u32;

        let mut body = vec![0x01, 0x00];
        body.extend_from_slice(&0x8004u16.to_le_bytes());
        body.extend_from_slice(&owner_offset.to_le_bytes());
        body.extend_from_slice(&group_offset.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&dacl_offset.to_le_bytes());
        body.extend_from_slice(&owner);
        body.extend_from_slice(&group);
        body.extend_from_slice(&dacl);

        let mut descriptor = (body.len() as u32).to_le_bytes().to_vec();
        descriptor.extend_from_slice(&body);
        descriptor
    }

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

        let dacl_ace = &res.dacl_aces[0];

        assert_eq!(dacl_ace.account_sid.as_deref(), Some("S-1-5-32-545"));
        assert_eq!(
            dacl_ace.ace_type.as_ref().unwrap(),
            "ACCESS_ALLOWED_ACE_TYPE"
        );
    }

    #[test]
    fn descriptor_decodes_multiple_dacl_aces() {
        let ace1 = access_ace(0x00, 0x00, 0x0002_0019, &sid_builtin_users());
        let ace2 = access_ace(0x01, 0x02, 0x0002_0006, &sid_builtin_admins());
        let descriptor = descriptor_with_dacl(&[ace1, ace2]);

        let res = security_descriptor_from_bytes(&descriptor).unwrap();

        assert_eq!(res.owner_sid, "S-1-5-32-544");
        assert_eq!(res.group_sid, "S-1-1-0");
        assert_eq!(res.dacl_aces.len(), 2);
        assert_eq!(
            res.dacl_aces[0].ace_type.as_deref(),
            Some("ACCESS_ALLOWED_ACE_TYPE")
        );
        assert_eq!(
            res.dacl_aces[0].account_sid.as_deref(),
            Some("S-1-5-32-545")
        );
        assert_eq!(
            res.dacl_aces[1].ace_type.as_deref(),
            Some("ACCESS_DENIED_ACE_TYPE")
        );
        assert_eq!(
            res.dacl_aces[1].ace_flags,
            vec!["CONTAINER_INHERIT_ACE".to_string()]
        );
        assert_eq!(
            res.dacl_aces[1].account_sid.as_deref(),
            Some("S-1-5-32-544")
        );
    }

    #[test]
    fn descriptor_to_record_emits_ace_arrays() {
        let ace = access_ace(0x00, 0x00, 0x0002_0019, &sid_builtin_users());
        let descriptor = descriptor_with_dacl(&[ace]);

        let record = security_descriptor_from_bytes(&descriptor)
            .unwrap()
            .to_record();

        assert!(matches!(record.get("sacl_aces"), Some(Value::Array(values)) if values.is_empty()));
        assert!(matches!(record.get("dacl_aces"), Some(Value::Array(values)) if values.len() == 1));
        assert!(record.get("sacl_ace").is_none());
        assert!(record.get("dacl_ace").is_none());
    }

    #[test]
    fn descriptor_short_inputs_do_not_panic() {
        for len in 0..=24 {
            let input = vec![0; len];
            let parsed = security_descriptor_from_bytes(&input);
            assert!(parsed.is_ok(), "len {len} returned {parsed:?}");
        }
    }

    #[test]
    fn descriptor_truncated_acl_header_returns_empty_ace_list() {
        let mut dacl = vec![0x02, 0x00, 0x08];
        let descriptor = descriptor_with_raw_dacl(std::mem::take(&mut dacl));

        let res = security_descriptor_from_bytes(&descriptor).unwrap();

        assert!(res.dacl_aces.is_empty());
    }

    #[test]
    fn descriptor_truncated_ace_body_returns_partial_ace() {
        let mut ace = access_ace(0x00, 0x00, 0x0002_0019, &sid_builtin_users());
        let full_size = read_u16_le(&ace, 2).unwrap();
        ace.truncate(10);

        let mut dacl = vec![0x02, 0x00];
        dacl.extend_from_slice(&(8u16 + full_size).to_le_bytes());
        dacl.extend_from_slice(&1u16.to_le_bytes());
        dacl.extend_from_slice(&0u16.to_le_bytes());
        dacl.extend_from_slice(&ace);

        let res = security_descriptor_from_bytes(&descriptor_with_raw_dacl(dacl)).unwrap();

        assert_eq!(res.dacl_aces.len(), 1);
        assert_eq!(
            res.dacl_aces[0].ace_type.as_deref(),
            Some("ACCESS_ALLOWED_ACE_TYPE")
        );
        assert_eq!(res.dacl_aces[0].ace_size, Some(full_size));
        assert_eq!(res.dacl_aces[0].account_sid, None);
        assert_eq!(res.dacl_aces[0].raw_hex, Some(hex::encode(ace)));
    }

    #[test]
    fn descriptor_unknown_ace_type_is_preserved() {
        let ace = access_ace(0xff, 0x80, 0x0002_0019, &sid_builtin_users());
        let descriptor = descriptor_with_dacl(std::slice::from_ref(&ace));

        let res = security_descriptor_from_bytes(&descriptor).unwrap();

        assert_eq!(res.dacl_aces.len(), 1);
        assert_eq!(
            res.dacl_aces[0].ace_type.as_deref(),
            Some("UNKNOWN_ACE_TYPE_0xFF")
        );
        assert_eq!(res.dacl_aces[0].ace_size, Some(ace.len() as u16));
        assert_eq!(
            res.dacl_aces[0].ace_flags,
            vec!["FAILED_ACCESS_ACE_FLAG".to_string()]
        );
        assert_eq!(res.dacl_aces[0].raw_hex, Some(hex::encode(ace)));
        assert_eq!(res.dacl_aces[0].account_sid, None);
    }

    #[test]
    fn descriptor_object_ace_decodes_guids() {
        let object_guid = [
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let inherited_guid = [
            0x98, 0xba, 0xdc, 0xfe, 0x54, 0x76, 0x10, 0x32, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ];
        let ace = object_ace(
            0x05,
            0x00,
            0x0002_0019,
            0x0000_0003,
            Some(object_guid),
            Some(inherited_guid),
            &sid_builtin_users(),
        );

        let res = security_descriptor_from_bytes(&descriptor_with_dacl(&[ace])).unwrap();

        assert_eq!(res.dacl_aces.len(), 1);
        assert_eq!(
            res.dacl_aces[0].ace_type.as_deref(),
            Some("ACCESS_ALLOWED_OBJECT_ACE_TYPE")
        );
        assert_eq!(
            res.dacl_aces[0].object_type_guid.as_deref(),
            Some("00112233-4455-6677-8899-aabbccddeeff")
        );
        assert_eq!(
            res.dacl_aces[0].inherited_object_type_guid.as_deref(),
            Some("fedcba98-7654-3210-0123-456789abcdef")
        );
        assert_eq!(
            res.dacl_aces[0].account_sid.as_deref(),
            Some("S-1-5-32-545")
        );
    }

    #[test]
    fn convert_sid_deterministic_truncation_cases() {
        assert_eq!(convert_sid(&[]).unwrap(), "S-1-");
        assert_eq!(convert_sid(&[0x01, 0x01]).unwrap(), "S-1-");
        assert_eq!(
            convert_sid(&[0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05]).unwrap(),
            "S-1-5"
        );
        assert_eq!(convert_sid(&sid_builtin_users()).unwrap(), "S-1-5-32-545");
    }
}
