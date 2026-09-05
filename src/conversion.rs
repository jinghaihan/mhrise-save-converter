use std::{
  fs,
  io::Read,
  path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use flate2::read::DeflateDecoder;
use murmur3::murmur3_32;

use crate::{
  crypto::Citrus,
  discover::{SaveFileKind, discover_save_files},
  format::{ChecksumStatus, DsssHeader, Platform, SaveFlags, checksum_status, parse_header},
  payload::SavePayload,
  translation::merge_onto_template,
};

const DSSS_HEADER_LEN: usize = 12;
const FILE_HASH_LEN: usize = 4;
const CITRUS_SIZE_FIELD_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
  NintendoSwitch,
  Steam,
}

#[derive(Debug, Clone, Copy)]
pub struct ConversionOptions {
  pub target: TargetPlatform,
  pub source_steamid64: Option<u64>,
  pub target_steamid64: Option<u64>,
  pub source_curve_index: Option<usize>,
  pub target_curve_index: Option<usize>,
}

pub fn convert_path(
  input: &Path,
  output: &Path,
  options: ConversionOptions,
  target_reference: Option<&Path>,
  force: bool,
) -> Result<Vec<PathBuf>> {
  let files = discover_save_files(input)?;
  let output_is_directory = input.is_dir();
  if output_is_directory {
    if output.exists() && !force {
      let mut entries = fs::read_dir(output)?;
      if entries.next().transpose()?.is_some() {
        bail!(
          "output directory {} is not empty; use --force to overwrite generated files",
          output.display()
        );
      }
    }
    fs::create_dir_all(output)
      .with_context(|| format!("could not create output directory {}", output.display()))?;
  } else if output.exists() && !force {
    bail!("output file {} already exists; use --force to overwrite it", output.display());
  } else if let Some(parent) = output.parent() {
    fs::create_dir_all(parent)?;
  }

  let mut options = options;
  let mut written = Vec::with_capacity(files.len());
  for file in files {
    if file.kind == SaveFileKind::Core
      && file.platform == Platform::Steam
      && options.source_curve_index.is_none()
    {
      let steamid64 =
        options.source_steamid64.context("Steam source requires --source-steamid64")?;
      options.source_curve_index =
        Some(find_curve_index(&file.path, steamid64).with_context(|| {
          format!("could not detect source Curve Index from {}", file.path.display())
        })?);
    }
    let output_path = if output_is_directory {
      output.join(file.path.file_name().expect("discovered file has a name"))
    } else {
      output.to_path_buf()
    };
    let data =
      fs::read(&file.path).with_context(|| format!("could not read {}", file.path.display()))?;
    if !matches!(checksum_status(&data)?, ChecksumStatus::Valid) {
      bail!("source file {} has an invalid checksum; refusing to convert it", file.path.display());
    }
    let converted = match file.kind {
      SaveFileKind::Core => {
        let target_template = target_reference
          .map(|reference| read_matching_reference(reference, &file.path))
          .transpose()?
          .flatten();
        convert_bytes_with_template(&data, target_template.as_deref(), options)
      }
      SaveFileKind::Auxiliary => {
        convert_auxiliary_bytes(&data, options.target, options.target_steamid64)
      }
    }
    .with_context(|| format!("could not convert {}", file.path.display()))?;
    fs::write(&output_path, converted)
      .with_context(|| format!("could not write {}", output_path.display()))?;
    written.push(output_path);
  }
  Ok(written)
}

fn read_matching_reference(reference: &Path, source_file: &Path) -> Result<Option<Vec<u8>>> {
  let reference_file = if reference.is_dir() {
    reference.join(source_file.file_name().context("source save file has no name")?)
  } else {
    reference.to_path_buf()
  };
  if !reference_file.exists() {
    return Ok(None);
  }
  fs::read(&reference_file)
    .with_context(|| format!("could not read target template {}", reference_file.display()))
    .map(Some)
}

pub fn convert_bytes(data: &[u8], options: ConversionOptions) -> Result<Vec<u8>> {
  convert_bytes_with_template(data, None, options)
}

pub fn convert_bytes_with_template(
  data: &[u8],
  target_template: Option<&[u8]>,
  options: ConversionOptions,
) -> Result<Vec<u8>> {
  let header = parse_header(data).context("invalid source DSSS header")?;
  if !matches!(checksum_status(data)?, ChecksumStatus::Valid) {
    bail!("source save has an invalid checksum; refusing to convert it");
  }
  let source_platform = header.platform();
  let mut payload =
    unpack_payload(data, header, options.source_steamid64, options.source_curve_index)?;
  if let Some(template) = target_template {
    let target_header = parse_header(template).context("invalid target-template DSSS header")?;
    if target_header.platform() != target_platform(options.target) {
      bail!(
        "target template is {}, but requested output is {}",
        target_header.platform(),
        target_platform(options.target)
      );
    }
    if header.platform() != target_header.platform() {
      let target_payload = unpack_payload(
        template,
        target_header,
        options.target_steamid64,
        options.target_curve_index,
      )?;
      let source = SavePayload::parse_at_offset(&payload, class_stream_offset(header.platform()))
        .context("invalid source class stream")?;
      let target = SavePayload::parse_at_offset(
        &target_payload,
        class_stream_offset(target_header.platform()),
      )
      .context("invalid target-template class stream")?;
      payload = merge_onto_template(&source, &target)
        .0
        .encode_at_offset(class_stream_offset(target_header.platform()))
        .context("could not encode translated class stream")?;
    }
  }
  pack_payload(&payload, options.target, options.target_steamid64, options.target_curve_index)
    .with_context(|| format!("cannot pack {} as target format", source_platform))
}

fn target_platform(target: TargetPlatform) -> Platform {
  match target {
    TargetPlatform::NintendoSwitch => Platform::NintendoSwitch,
    TargetPlatform::Steam => Platform::Steam,
  }
}

fn class_stream_offset(platform: Platform) -> usize {
  match platform {
    Platform::NintendoSwitch => DSSS_HEADER_LEN,
    Platform::Steam => align_up(DSSS_HEADER_LEN, 16),
    Platform::Auxiliary | Platform::Unknown => 0,
  }
}

pub fn convert_auxiliary_bytes(
  data: &[u8],
  target: TargetPlatform,
  target_steamid64: Option<u64>,
) -> Result<Vec<u8>> {
  let header = parse_header(data).context("invalid auxiliary DSSS header")?;
  if !matches!(checksum_status(data)?, ChecksumStatus::Valid) {
    bail!("auxiliary save has an invalid checksum; refusing to convert it");
  }

  let source_flags = header.raw_flags;
  let source_payload_offset = match source_flags {
    0 => DSSS_HEADER_LEN,
    value if value == SaveFlags::HAS_ID.bits() => align_up(DSSS_HEADER_LEN, 8) + 8,
    value => bail!("unsupported auxiliary DSSS flags 0x{value:08x}"),
  };
  let payload_end = data.len().checked_sub(FILE_HASH_LEN).context("auxiliary save is too small")?;
  if payload_end < source_payload_offset {
    bail!("auxiliary save payload exceeds file bounds");
  }

  let mut output = Vec::with_capacity(data.len() + 12);
  output.extend_from_slice(b"DSSS");
  output.extend_from_slice(&2u32.to_le_bytes());
  match target {
    TargetPlatform::NintendoSwitch => {
      output.extend_from_slice(&SaveFlags::empty().bits().to_le_bytes());
    }
    TargetPlatform::Steam => {
      output.extend_from_slice(&SaveFlags::HAS_ID.bits().to_le_bytes());
      output.resize(align_up(output.len(), 8), 0);
      let id = target_steamid64.context("Steam target requires --target-steamid64")?;
      output.extend_from_slice(&(id & u32::MAX as u64).to_le_bytes());
    }
  }
  output.extend_from_slice(&data[source_payload_offset..payload_end]);
  finish_file(output)
}

pub fn find_curve_index(path: &Path, steamid64: u64) -> Result<usize> {
  let data = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
  let header = parse_header(&data).context("invalid target DSSS header")?;
  if header.platform() != Platform::Steam {
    bail!("target reference is not a Steam Citrus save");
  }
  let (encrypted, decrypted_len) = citrus_payload(&data)?;
  let citrus = Citrus::new(steamid64, None);
  let params = citrus
    .brute_force_find_params(encrypted, decrypted_len)
    .context("could not determine Citrus Curve Index")?;
  Ok(params.index as usize)
}

fn unpack_payload(
  data: &[u8],
  header: DsssHeader,
  steamid64: Option<u64>,
  curve_index: Option<usize>,
) -> Result<Vec<u8>> {
  match header.platform() {
    Platform::NintendoSwitch => unpack_deflate(data),
    Platform::Steam => {
      let id = steamid64.context("Steam source requires --source-steamid64")?;
      let (encrypted, decrypted_len) = citrus_payload(data)?;
      Citrus::new(id, curve_index)
        .decrypt(encrypted, decrypted_len)
        .context("Citrus decryption failed; check the source SteamID64 and curve index")
    }
    other => bail!("{} is not a supported core save format", other),
  }
}

fn unpack_deflate(data: &[u8]) -> Result<Vec<u8>> {
  let metadata_offset = align_up(DSSS_HEADER_LEN, 8);
  if data.len() < metadata_offset + 24 + FILE_HASH_LEN {
    bail!("Switch save is missing its DEFLATE metadata");
  }

  let compressed_size = read_u32(data, metadata_offset + 12)? as usize;
  let decompressed_size = read_u64(data, metadata_offset + 16)? as usize;
  let compressed_offset = metadata_offset + 24;
  let compressed_end = compressed_offset
    .checked_add(compressed_size)
    .context("Switch compressed payload length overflow")?;
  if compressed_end > data.len() - FILE_HASH_LEN {
    bail!("Switch compressed payload exceeds file bounds");
  }

  let mut decoder = DeflateDecoder::new(&data[compressed_offset..compressed_end]);
  let mut payload = Vec::with_capacity(decompressed_size);
  decoder.read_to_end(&mut payload)?;
  if payload.len() != decompressed_size {
    bail!("Switch DEFLATE size mismatch: expected {decompressed_size}, got {}", payload.len());
  }
  Ok(payload)
}

fn citrus_payload(data: &[u8]) -> Result<(&[u8], usize)> {
  let payload_offset = align_up(DSSS_HEADER_LEN, 16);
  if data.len() < payload_offset + CITRUS_SIZE_FIELD_LEN + FILE_HASH_LEN {
    bail!("Steam save is missing its Citrus payload");
  }
  let size_offset = data.len() - FILE_HASH_LEN - CITRUS_SIZE_FIELD_LEN;
  let decrypted_len = read_u64(data, size_offset)? as usize;
  Ok((&data[payload_offset..size_offset], decrypted_len))
}

fn pack_payload(
  payload: &[u8],
  target: TargetPlatform,
  steamid64: Option<u64>,
  curve_index: Option<usize>,
) -> Result<Vec<u8>> {
  let mut output = Vec::new();
  match target {
    TargetPlatform::NintendoSwitch => {
      output.extend_from_slice(b"DSSS");
      output.extend_from_slice(&2u32.to_le_bytes());
      output.extend_from_slice(&SaveFlags::DEFLATE.bits().to_le_bytes());
      output.resize(align_up(output.len(), 8), 0);

      let compressed = compress_deflate(payload)?;
      output.extend_from_slice(&((compressed.len() as u64) + 0x10).to_le_bytes());
      output.extend_from_slice(&1u32.to_le_bytes());
      output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
      output.extend_from_slice(&(payload.len() as u64).to_le_bytes());
      output.extend_from_slice(&compressed);
    }
    TargetPlatform::Steam => {
      let id = steamid64.context("Steam target requires --target-steamid64")?;
      let curve =
        curve_index.context("Steam target requires --target-curve-index or --target-reference")?;
      output.extend_from_slice(b"DSSS");
      output.extend_from_slice(&2u32.to_le_bytes());
      output.extend_from_slice(&SaveFlags::CITRUS.bits().to_le_bytes());
      output.resize(align_up(output.len(), 16), 0);
      let encrypted = Citrus::new(id, Some(curve))
        .encrypt(payload)
        .context("Citrus encryption failed; check the target Curve Index")?;
      output.extend_from_slice(&encrypted);
      output.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    }
  }

  finish_file(output)
}

fn finish_file(mut output: Vec<u8>) -> Result<Vec<u8>> {
  output.resize(align_up(output.len(), 4), 0);
  let hash = murmur3_32(&mut std::io::Cursor::new(&output), 0xffff_ffff)?;
  output.extend_from_slice(&hash.to_le_bytes());
  Ok(output)
}

fn compress_deflate(payload: &[u8]) -> Result<Vec<u8>> {
  use flate2::{Compression, write::DeflateEncoder};
  use std::io::Write;

  let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(5));
  encoder.write_all(payload)?;
  Ok(encoder.finish()?)
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
  let bytes = data.get(offset..offset + 4).context("unexpected end of file while reading u32")?;
  Ok(u32::from_le_bytes(bytes.try_into().expect("fixed-size slice")))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64> {
  let bytes = data.get(offset..offset + 8).context("unexpected end of file while reading u64")?;
  Ok(u64::from_le_bytes(bytes.try_into().expect("fixed-size slice")))
}

fn align_up(value: usize, alignment: usize) -> usize {
  value.div_ceil(alignment) * alignment
}

pub fn verify_file(path: &Path) -> Result<bool> {
  let data = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
  let _ = parse_header(&data)?;
  Ok(matches!(checksum_status(&data)?, crate::format::ChecksumStatus::Valid))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::payload::{Class, Field, FieldValue, NativeClass, SavePayload};

  const TEST_STEAM_ID: u64 = 76_561_198_382_766_028;

  #[test]
  fn switch_steam_switch_roundtrip_preserves_payload() {
    let payload = b"MHRise save payload used for a conversion round trip";
    let switch = pack_payload(payload, TargetPlatform::NintendoSwitch, None, None)
      .expect("Switch packing should succeed");
    let steam = convert_bytes(
      &switch,
      ConversionOptions {
        target: TargetPlatform::Steam,
        source_steamid64: None,
        target_steamid64: Some(TEST_STEAM_ID),
        source_curve_index: None,
        target_curve_index: Some(0),
      },
    )
    .expect("Switch to Steam conversion should succeed");
    let roundtrip = convert_bytes(
      &steam,
      ConversionOptions {
        target: TargetPlatform::NintendoSwitch,
        source_steamid64: Some(TEST_STEAM_ID),
        target_steamid64: None,
        source_curve_index: Some(0),
        target_curve_index: None,
      },
    )
    .expect("Steam to Switch conversion should succeed");

    assert_eq!(unpack_deflate(&roundtrip).expect("roundtrip should deflate"), payload);
    assert!(matches!(checksum_status(&roundtrip), Ok(ChecksumStatus::Valid)));
  }

  #[test]
  fn conversion_rejects_invalid_outer_checksum() {
    let mut switch = pack_payload(b"payload", TargetPlatform::NintendoSwitch, None, None)
      .expect("Switch packing should succeed");
    switch[20] ^= 0xff;

    let error = convert_bytes(
      &switch,
      ConversionOptions {
        target: TargetPlatform::NintendoSwitch,
        source_steamid64: None,
        target_steamid64: None,
        source_curve_index: None,
        target_curve_index: None,
      },
    )
    .expect_err("invalid source checksum should be rejected");

    assert!(error.to_string().contains("invalid checksum"));
  }

  #[test]
  fn auxiliary_wrapper_roundtrip_preserves_payload() {
    let mut switch = Vec::from(&b"DSSS"[..]);
    switch.extend_from_slice(&2u32.to_le_bytes());
    switch.extend_from_slice(&SaveFlags::empty().bits().to_le_bytes());
    switch.extend_from_slice(b"auxiliary payload with album data");
    let switch = finish_file(switch).expect("Switch auxiliary fixture should be valid");
    let steam_id = 76_561_198_382_766_028u64;

    let steam = convert_auxiliary_bytes(&switch, TargetPlatform::Steam, Some(steam_id))
      .expect("Switch auxiliary conversion should succeed");
    assert_eq!(&steam[8..12], &SaveFlags::HAS_ID.bits().to_le_bytes());
    assert_eq!(&steam[16..24], &(steam_id & u32::MAX as u64).to_le_bytes());

    let roundtrip = convert_auxiliary_bytes(&steam, TargetPlatform::NintendoSwitch, None)
      .expect("Steam auxiliary conversion should succeed");
    assert_eq!(roundtrip, switch);
  }

  #[test]
  fn cross_platform_conversion_uses_target_template_schema() {
    let source_payload = SavePayload {
      entries: vec![NativeClass {
        native_hash: 1,
        class: Class {
          hash: 10,
          fields: vec![Field {
            hash: 100,
            field_type: 8,
            value: FieldValue::Scalar { size: 4, bytes: 7u32.to_le_bytes().to_vec() },
          }],
        },
      }],
    }
    .encode_at_offset(DSSS_HEADER_LEN)
    .expect("source payload should encode");
    let target_payload = SavePayload {
      entries: vec![NativeClass {
        native_hash: 1,
        class: Class {
          hash: 10,
          fields: vec![
            Field {
              hash: 100,
              field_type: 8,
              value: FieldValue::Scalar { size: 4, bytes: 9u32.to_le_bytes().to_vec() },
            },
            Field {
              hash: 101,
              field_type: 8,
              value: FieldValue::Scalar { size: 4, bytes: 11u32.to_le_bytes().to_vec() },
            },
          ],
        },
      }],
    }
    .encode_at_offset(align_up(DSSS_HEADER_LEN, 16))
    .expect("target payload should encode");
    let source = pack_payload(&source_payload, TargetPlatform::NintendoSwitch, None, None)
      .expect("Switch source should pack");
    let target = pack_payload(&target_payload, TargetPlatform::Steam, Some(TEST_STEAM_ID), Some(0))
      .expect("Steam template should pack");

    let converted = convert_bytes_with_template(
      &source,
      Some(&target),
      ConversionOptions {
        target: TargetPlatform::Steam,
        source_steamid64: None,
        target_steamid64: Some(TEST_STEAM_ID),
        source_curve_index: None,
        target_curve_index: Some(0),
      },
    )
    .expect("template conversion should succeed");
    let converted_header = parse_header(&converted).expect("converted header should parse");
    let converted_payload =
      unpack_payload(&converted, converted_header, Some(TEST_STEAM_ID), Some(0))
        .expect("converted payload should decrypt");
    let converted_payload =
      SavePayload::parse(&converted_payload).expect("converted payload should parse");

    assert_eq!(converted_payload.entries[0].class.fields.len(), 2);
    assert_eq!(
      converted_payload.entries[0].class.fields[0].value,
      FieldValue::Scalar { size: 4, bytes: 7u32.to_le_bytes().to_vec() }
    );
    assert_eq!(
      converted_payload.entries[0].class.fields[1].value,
      FieldValue::Scalar { size: 4, bytes: 11u32.to_le_bytes().to_vec() }
    );
  }
}
