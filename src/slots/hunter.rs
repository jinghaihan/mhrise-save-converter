//! Portable single-character ZIP containers. Core payloads are not account-encrypted.
use super::*;
use crate::{conversion::convert_auxiliary_bytes, payload::NativeClass};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const MAX_ENTRIES: usize = 1024;
const MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
  format: String,
  version: u32,
  platform: String,
  source_slot: u8,
  source_steamid64: Option<u64>,
}

/// Export exactly one occupied hunter, its selection-screen summary and known photos.
/// Existing output files and paths inside the source directory are never overwritten.
pub fn export_hunter(input: &Path, output: &Path, slot: u8, options: SlotOptions) -> Result<()> {
  ensure!((1..=3).contains(&slot), "slot must be 1–3");
  let source = fs::canonicalize(input)?;
  let bundle = load_bundle(&source, options)?;
  let (plain, _) = bundle.roles.get(&slot).context("cannot export an empty slot")?;
  let FieldValue::Array(hunters) = &field(&bundle.system, LOAD_INFO, HUNTERS)?.value else {
    bail!("hunter summaries are not an array")
  };
  let ArrayValue::Class(summary) = &hunters.values[usize::from(slot - 1)] else {
    bail!("invalid hunter summary")
  };
  let summary =
    SavePayload { entries: vec![NativeClass { native_hash: 0, class: (**summary).clone() }] }
      .encode()?;
  let manifest = Manifest {
    format: "mhrise-hunter".into(),
    version: 1,
    platform: match bundle.inspection.platform {
      Platform::Steam => "steam",
      _ => "switch",
    }
    .into(),
    source_slot: slot,
    source_steamid64: if bundle.inspection.platform == Platform::Steam {
      bundle.options.steamid64
    } else {
      None
    },
  };
  let mut entries = BTreeMap::from([
    ("manifest.json".to_owned(), serde_json::to_vec_pretty(&manifest)?),
    ("role.bin".to_owned(), plain.clone()),
    ("summary.bin".to_owned(), summary),
  ]);
  let prefix = format!("SS{slot}_");
  for (name, bytes) in &bundle.files {
    if let Some(suffix) = name.strip_prefix(&prefix) {
      ensure!(
        is_auxiliary_filename(name),
        "unsupported album file {name}; refusing to silently omit it"
      );
      entries.insert(
        format!("photos/{suffix}"),
        convert_auxiliary_bytes(bytes, TargetPlatform::NintendoSwitch, None)?,
      );
    }
  }
  ensure!(
    entries.len() <= MAX_ENTRIES
      && entries.values().map(|v| v.len() as u64).sum::<u64>() <= MAX_BYTES,
    "hunter package exceeds safety limits"
  );
  let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
  for (name, bytes) in entries {
    zip.start_file(
      name,
      SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    zip.write_all(&bytes)?;
  }
  let bytes = zip.finish()?.into_inner();
  let parent =
    output.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
  let parent = fs::canonicalize(parent).context("output parent directory must already exist")?;
  ensure!(!parent.starts_with(source), "export must be outside the source save directory");
  let mut file = fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(output)
    .context("export path already exists or cannot be created")?;
  if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
    drop(file);
    let _ = fs::remove_file(output);
    return Err(error.into());
  }
  Ok(())
}
