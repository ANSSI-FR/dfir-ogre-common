# Raw MFT fixture

`tiny.mft.gz` contains only the raw `$MFT` stream (262144 bytes), without a
boot sector, disk image, journal or other metadata file. Tests decompress it
before calling the standalone MFT parser.

- Source: https://github.com/SecurityRonin/ntfs-forensic/blob/d1e95bce2404bb685b7f2aee26052d94174c7017/tests/data/SampleTinyNtfsVolume.zip
- Upstream provenance: https://github.com/SecurityRonin/ntfs-forensic/blob/d1e95bce2404bb685b7f2aee26052d94174c7017/tests/data/README.md
- Original sample: Joakim Schicht's LogFileParser v2.0.0.53, SampleTinyNtfsVolume.
- Archive SHA-256: b39098444d6114610f414d6a5318c850bb30b98d56de2673f427377352cd1ac7
- Raw MFT SHA-256: c837ffd998066086ed4a80606fba53711fd186d3a6de3c6992f193868fce137a
- Gzip SHA-256: 9aa472542986732d30577334449a56b44a01a2a7f8d4487b3fb885306dccb971
- Extraction: unzip `SampleTinyNtfsVolume/partition.dd`; its 512-byte clusters
  hold the MFT in two `$DATA` runs: LCN 4778, 32 clusters, followed by LCN 359,
  480 clusters. Concatenate these ranges in that order, keeping 262144 bytes.
  Equivalent Sleuth Kit command: `icat partition.dd 0 > tiny.mft`.
- Compression: Python `gzip.compress(raw_mft_bytes, mtime=0)`.
- Known sample record: MFT entry 32 is `file8.txt` (also documented by the
  upstream deleted-node test); sample metadata and timestamps are synthetic.

The original sample is redistributed under the MIT license, copyright
Joakim Schicht; see `LICENSE.fixture`. No acquired investigation data is included.
