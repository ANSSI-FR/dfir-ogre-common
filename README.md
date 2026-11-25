# DFIR-OGRE-COMMON

[![badge_repo](https://img.shields.io/badge/ANSSI--FR-DFIR--Ogre-white)](https://github.com/ANSSI-FR/dfir-ogre)
[![category_badge_external](https://img.shields.io/badge/category-external-%23b556b6)](https://anssi-fr.github.io/README.en.html#project-categories)
[![openess_badge_A](https://img.shields.io/badge/code.gouv.fr-collaborative-blue)](https://anssi-fr.github.io/README.en.html#openness-level)

This project is part of the [DFIR-OGRE](https://github.com/ANSSI-FR/dfir-ogre) suite and provides a low level Rust runtime to run DFIR-OGRE plugins.

Please refers to the ogre documentation for more details.

## Unsigned integers

The runtime supports signed `Value::Int(i64)` and unsigned `Value::UInt(u64)`
integers. JSON, JSONL and EVTX values up to `i64::MAX` retain the signed
representation; larger positive integers through `18446744073709551615`
(`u64::MAX`) are preserved as unsigned values. They are written as exact decimal
numbers in JSON/JSONL and CSV, including nested data and timeline output.

Use `parser="UInt"` for an explicitly unsigned decimal field in plugin XML:

```xml
<field input="counter" output="counter" parser="UInt" />
```

`Parser.UInt()` and `Value.UInt(value)` expose the corresponding Python types.
UInt accepts values from zero through `2**64 - 1`; Int remains signed, from
`-2**63` through `2**63 - 1`. Explicit integer mappings validate native JSON
numbers as well as strings, including mapped array elements. Negative,
fractional and overflowing values fail UInt parsing. Missing and empty text
fields follow the existing null/default conventions, and UInt can be used in
Dynamic parsers. In `multi_input`, integer children are checked before joining;
valid source text, including leading zeros, is preserved, and missing, null or
empty inputs use a declared default. A null native JSON scalar remains null.
`default_parser` remains limited to String or Ignore.

Small integer values keep their existing generated identifiers. A large UInt
has a distinct canonical representation from a wrapped negative value, such as
`-1`. Artifact-specific sentinel interpretation belongs in the relevant mapping.
JSON readers must support these integer ranges to preserve precision; readers
that convert all numbers to floating point can round the values even though
the exported decimal text is exact.

## SRUM parser migration

SRUM parsing now lives in `dfir-ogre-plugin-windows` and uses that package's
`libesedb-python` dependency. The `Srum` plugin command and its XML configuration
remain unchanged. Upgrade the Windows plugin before or together with this common
version: older plugin versions import the removed common SRUM function.

Direct Python callers should replace:

```python
from dfir_ogre_common import parse_srum
```

with:

```python
from dfir_ogre_plugin_windows.srum import parse_srum
```

The four arguments and `RunReport` result are unchanged. The Rust
`parser::srum` module and `dfir_ogre_common::parse_srum` export have been removed;
Rust-only consumers must invoke the Windows Python plugin or retain the older
common version. Common no longer bundles the `libesedb` C engine.

## ⚠️ Beta Status
This software is currently in **beta**. While functional and actively developed, it may still undergo breaking changes, and some artefact parsers may not yet be fully stabilized. We welcome feedback, bug reports, and contributions using the [issue tracker](https://github.com/ANSSI-FR/dfir-ogre/issues).


## French Cybersecurity Agency (ANSSI)

<img src="https://www.sgdsn.gouv.fr/files/styles/ds_image_paragraphe/public/files/Notre_Organisation/logo_anssi.png" alt="ANSSI logo" width="25%">

*This projet is managed by [ANSSI](https://cyber.gouv.fr/). To find out more, you can visit the [page](https://cyber.gouv.fr/enjeux-technologiques/open-source/) (in French) dedicated to ANSSI’s open-source strategy. You can also click on the badges above to learn more about their meaning.*
