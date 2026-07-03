# Security Descriptor Robustness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Windows security descriptor parsing panic-free, deterministic, and able to preserve all SACL/DACL ACEs, including partial and unknown ACEs.

**Architecture:** Keep `security_descriptor_from_bytes` as the public API, but move all binary reads behind bounded helper functions. Parse security descriptor header fields, ACLs, ACEs, SIDs, rights, flags, and object GUIDs independently so malformed bytes only drop the affected substructure. Change the output model from one optional ACE per ACL to arrays of ACE objects.

**Tech Stack:** Rust 2024, PyO3, inline Rust unit tests, existing `hex` crate, existing XML field mapping format.

---

## File Structure

- Modify `src/windows_utils.rs`: security descriptor structs, record conversion, bounded binary helpers, ACL parsing, ACE parsing, deterministic flag decoding, GUID formatting, and unit tests.
- Modify `dfir_ogre_common.pyi`: Python type hints for plural ACE arrays and optional/new ACE fields.
- Modify `test_data/hive/hive.xml`: map `descriptor.sacl_aces` and `descriptor.dacl_aces` as arrays of ACE objects.
- No new runtime dependencies.
- No Python tests.

## Task 1: Prove and Fix the Short Descriptor Panic

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Add the deterministic short-input regression test**

Add this test inside `#[cfg(test)] mod tests` in `src/windows_utils.rs`, before the existing randomized tests:

```rust
#[test]
fn descriptor_short_inputs_do_not_panic() {
    for len in 0..=24 {
        let input = vec![0; len];
        let parsed = security_descriptor_from_bytes(&input);
        assert!(parsed.is_ok(), "len {len} returned {parsed:?}");
    }
}
```

- [ ] **Step 2: Run the regression test and verify it fails**

Run:

```bash
cargo test windows_utils::tests::descriptor_short_inputs_do_not_panic
```

Expected: FAIL with an out-of-bounds slice panic around the old `b[12..16]` group offset read.

- [ ] **Step 3: Add the minimal bound check for the known panic**

In `security_descriptor_from_bytes`, directly before the existing `group_offset` read, add:

```rust
    if b.len() < 16 {
        return Ok(descriptor);
    }
```

The code around the group offset should become:

```rust
    if b.len() < 16 {
        return Ok(descriptor);
    }
    let group_offset = u32::from_le_bytes(
        b[12..16]
            .try_into()
            .map_err(|_| Error::InvalidByteLenght("group offset".into()))?,
    ) as usize;
```

- [ ] **Step 4: Run the regression test and verify it passes**

Run:

```bash
cargo test windows_utils::tests::descriptor_short_inputs_do_not_panic
```

Expected: PASS.

- [ ] **Step 5: Commit the focused panic fix**

Run:

```bash
git add src/windows_utils.rs
git commit -m "Fix short security descriptor panic"
```

## Task 2: Add Deterministic Test Builders and Schema Tests

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Replace randomized test setup with deterministic helpers**

Inside `#[cfg(test)] mod tests`, remove:

```rust
use rand::random;
```

Add these helper functions at the top of the test module:

```rust
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
    vec![0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]
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
```

- [ ] **Step 2: Add tests for the approved plural ACE schema**

Add these tests after the existing `descriptor` test:

```rust
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

    let record = security_descriptor_from_bytes(&descriptor).unwrap().to_record();

    assert!(matches!(record.get("sacl_aces"), Some(Value::Array(values)) if values.is_empty()));
    assert!(matches!(record.get("dacl_aces"), Some(Value::Array(values)) if values.len() == 1));
    assert!(record.get("sacl_ace").is_none());
    assert!(record.get("dacl_ace").is_none());
}
```

- [ ] **Step 3: Run the new tests and verify they fail to compile**

Run:

```bash
cargo test windows_utils::tests::descriptor_to_record_emits_ace_arrays
```

Expected: FAIL to compile because `SecurityDescriptor` does not yet have `dacl_aces` and because `SecurityDescriptorAce.account_sid` is not optional.

## Task 3: Change the Rust Data Model and Record Output

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Replace the security descriptor structs**

Replace the current `SecurityDescriptor` and `SecurityDescriptorAce` definitions with:

```rust
#[derive(Debug, Clone, Default)]
#[pyclass(get_all, from_py_object)]
pub struct SecurityDescriptor {
    control_flags: Vec<String>,
    owner_sid: String,
    group_sid: String,
    sacl_aces: Vec<SecurityDescriptorAce>,
    dacl_aces: Vec<SecurityDescriptorAce>,
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
```

- [ ] **Step 2: Replace `SecurityDescriptor::to_record`**

Use this implementation:

```rust
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
```

- [ ] **Step 3: Replace `SecurityDescriptorAce::to_record`**

Use this implementation:

```rust
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
```

- [ ] **Step 4: Temporarily update existing parser assignments for plural fields**

In `security_descriptor_from_bytes`, replace:

```rust
descriptor.sacl_ace = Some(ace);
```

with:

```rust
descriptor.sacl_aces.push(ace);
```

and replace:

```rust
descriptor.dacl_ace = Some(ace);
```

with:

```rust
descriptor.dacl_aces.push(ace);
```

- [ ] **Step 5: Wrap existing ACE SID assignments in `Some`**

In the current `ace_from_bytes`, replace both copies of:

```rust
sec_ace.account_sid = account_sid;
```

with:

```rust
sec_ace.account_sid = Some(account_sid);
```

- [ ] **Step 6: Update the existing valid descriptor test for the plural schema**

In the `descriptor` test, replace:

```rust
let dacl_ace = &res.dacl_ace.unwrap();

assert_eq!(dacl_ace.account_sid, "S-1-5-32-545");
```

with:

```rust
let dacl_ace = &res.dacl_aces[0];

assert_eq!(dacl_ace.account_sid.as_deref(), Some("S-1-5-32-545"));
```

- [ ] **Step 7: Run the schema tests**

Run:

```bash
cargo test windows_utils::tests::descriptor_to_record_emits_ace_arrays
cargo test windows_utils::tests::descriptor -- --exact
```

Expected: PASS.

- [ ] **Step 8: Commit the schema model change**

Run:

```bash
git add src/windows_utils.rs
git commit -m "Represent descriptor ACLs as ACE arrays"
```

## Task 4: Replace Direct Binary Reads with Bounded Helpers

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Add ordered decode tables and bounded helpers**

Near the top of `src/windows_utils.rs`, after the `use` block, add:

```rust
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
```

- [ ] **Step 2: Replace the descriptor header, owner, and group parsing**

Replace the start of `security_descriptor_from_bytes` through the group SID assignment with:

```rust
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
```

Keep the existing SACL and DACL blocks below this for now, but update their presence checks:

```rust
    if has_flag(&descriptor.control_flags, "SE_SACL_PRESENT") {
```

and:

```rust
    if has_flag(&descriptor.control_flags, "SE_DACL_PRESENT") {
```

- [ ] **Step 3: Run the short-input test**

Run:

```bash
cargo test windows_utils::tests::descriptor_short_inputs_do_not_panic
```

Expected: PASS.

- [ ] **Step 4: Commit the bounded descriptor header parsing**

Run:

```bash
git add src/windows_utils.rs
git commit -m "Add bounded descriptor header parsing"
```

## Task 5: Parse ACLs as Independent ACEs

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Add the ACE type name and ACL parsing helpers**

Add these helpers before `security_descriptor_from_bytes`:

```rust
fn ace_type_name(ace_type: u8) -> String {
    ACE_BTYPE
        .get(&ace_type)
        .map_or_else(|| format!("UNKNOWN_ACE_TYPE_0x{ace_type:02X}"), |value| (*value).to_string())
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
            ace.ace_type = Some(ace_type_name(ace_type).to_string());
            ace.raw_hex = Some(hex::encode(partial));
            aces.push(ace);
            break;
        };

        if ace_size < 4 {
            let partial = slice_at(bytes, current, bytes.len().saturating_sub(current)).unwrap_or_default();
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
```

- [ ] **Step 2: Replace the SACL and DACL blocks in `security_descriptor_from_bytes`**

Replace both existing blocks with:

```rust
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
```

- [ ] **Step 3: Run the multiple-ACE test**

Run:

```bash
cargo test windows_utils::tests::descriptor_decodes_multiple_dacl_aces
```

Expected: PASS.

- [ ] **Step 4: Commit independent ACL parsing**

Run:

```bash
git add src/windows_utils.rs
git commit -m "Parse security descriptor ACL entries independently"
```

## Task 6: Make ACE Parsing Robust and Deterministic

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Add GUID and object ACE helpers**

Add these helpers near the existing `ace_type_name` helper:

```rust
fn is_known_ace_type(ace_type: u8) -> bool {
    ACE_BTYPE.contains_key(&ace_type)
}

fn is_object_ace_type(ace_type: u8) -> bool {
    matches!(ace_type, 0x05 | 0x06 | 0x07 | 0x08 | 0x0b | 0x0c | 0x0f | 0x10)
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
```

- [ ] **Step 2: Replace `ace_from_bytes` with a wrapper and robust implementation**

Replace the full current `ace_from_bytes` function with:

```rust
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
```

- [ ] **Step 3: Remove `HashSet` and the replaced unordered static maps**

Change:

```rust
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};
```

to:

```rust
use std::{collections::HashMap, sync::LazyLock};
```

Delete the complete static declarations named `ACE_BFLAGS`, `BRIGHTS`, and `SD_BFLAGS`; their data is now represented by `ACE_BFLAGS_ORDERED`, `BRIGHTS_ORDERED`, and `SD_BFLAGS_ORDERED`.

- [ ] **Step 4: Run descriptor tests**

Run:

```bash
cargo test windows_utils::tests::descriptor
```

Expected: PASS.

- [ ] **Step 5: Commit robust ACE parsing**

Run:

```bash
git add src/windows_utils.rs
git commit -m "Parse security descriptor ACEs robustly"
```

## Task 7: Add Deterministic Tests for Unknown, Truncated, and Object ACEs

**Files:**
- Modify: `src/windows_utils.rs`

- [ ] **Step 1: Delete the randomized fuzzing tests**

Remove these test functions:

```rust
fn descriptor_fuzzing()
fn ace_fuzzing()
fn convert_sid_fuzzing()
```

- [ ] **Step 2: Add deterministic malformed-input tests**

Add these tests to `src/windows_utils.rs`:

```rust
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
```

- [ ] **Step 3: Run the deterministic security descriptor tests**

Run:

```bash
cargo test windows_utils::tests::
```

Expected: PASS.

- [ ] **Step 4: Commit the deterministic test suite**

Run:

```bash
git add src/windows_utils.rs
git commit -m "Add deterministic security descriptor tests"
```

## Task 8: Update Python Type Hints and Hive XML Mapping

**Files:**
- Modify: `dfir_ogre_common.pyi`
- Modify: `test_data/hive/hive.xml`

- [ ] **Step 1: Update `SecurityDescriptor` in `dfir_ogre_common.pyi`**

Replace:

```python
self.sacl_ace: Optional["SecurityDescriptorAce"] = None
self.dacl_ace: Optional["SecurityDescriptorAce"] = None
```

with:

```python
self.sacl_aces: List["SecurityDescriptorAce"] = []
self.dacl_aces: List["SecurityDescriptorAce"] = []
```

- [ ] **Step 2: Update `SecurityDescriptorAce` in `dfir_ogre_common.pyi`**

Replace:

```python
self.account_sid: str = ""
```

with:

```python
self.account_sid: Optional[str] = None
self.ace_size: Optional[int] = None
self.object_type_guid: Optional[str] = None
self.inherited_object_type_guid: Optional[str] = None
self.raw_hex: Optional[str] = None
```

- [ ] **Step 3: Update the hive XML descriptor mapping**

In `test_data/hive/hive.xml`, replace both singular ACE object blocks with:

```xml
        <array>
          <object input="sacl_aces" ignore="false">
            <field input="ace_type" parser="String" />
            <array>
              <field input="ace_flags" parser="String" />
            </array>
            <array>
              <field input="rights" parser="String" />
            </array>
            <field input="account_sid" parser="String" />
            <field input="ace_size" parser="Int" />
            <field input="object_type_guid" parser="String" />
            <field input="inherited_object_type_guid" parser="String" />
            <field input="raw_hex" parser="String" />
          </object>
        </array>
        <array>
          <object input="dacl_aces" ignore="false">
            <field input="ace_type" parser="String" />
            <array>
              <field input="ace_flags" parser="String" />
            </array>
            <array>
              <field input="rights" parser="String" />
            </array>
            <field input="account_sid" parser="String" />
            <field input="ace_size" parser="Int" />
            <field input="object_type_guid" parser="String" />
            <field input="inherited_object_type_guid" parser="String" />
            <field input="raw_hex" parser="String" />
          </object>
        </array>
```

- [ ] **Step 4: Run hive parser tests**

Run:

```bash
cargo test parser::hive
```

Expected: PASS.

- [ ] **Step 5: Commit schema integration updates**

Run:

```bash
git add dfir_ogre_common.pyi test_data/hive/hive.xml
git commit -m "Update security descriptor schema mappings"
```

## Task 9: Full Verification

**Files:**
- Verify: all modified files

- [ ] **Step 1: Run rustfmt check**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Run strict Clippy**

Run:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: PASS.

- [ ] **Step 3: Run the full test suite**

Run:

```bash
LD_LIBRARY_PATH=/home/asalais/.local/share/uv/python/cpython-3.13.5-linux-x86_64-gnu/lib cargo test
```

Expected: PASS.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git diff --stat main...HEAD
```

Expected: The diff includes the design commit, plan commit, `src/windows_utils.rs`, `dfir_ogre_common.pyi`, and `test_data/hive/hive.xml`.

- [ ] **Step 5: Commit any verification-only formatting changes**

If `cargo fmt` changed files, run:

```bash
git add src/windows_utils.rs
git commit -m "Format security descriptor parser"
```

If `cargo fmt --check` passed without changes, skip this step.
