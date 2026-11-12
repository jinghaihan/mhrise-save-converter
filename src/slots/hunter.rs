//! Portable single-character ZIP containers. Core payloads are not account-encrypted.
use super::*;
use crate::{conversion::convert_auxiliary_bytes, payload::NativeClass};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

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

struct HunterPackage {
  manifest: Manifest,
  platform: Platform,
  plain: Vec<u8>,
  role: SavePayload,
  summary: crate::payload::Class,
  photos: BTreeMap<String, Vec<u8>>,
}

impl HunterPackage {
  fn read(path: &Path) -> Result<Self> {
    let file = fs::File::open(path).context("could not open hunter package")?;
    ensure!(file.metadata()?.len() <= MAX_BYTES, "hunter package is too large");
    let mut archive = ZipArchive::new(file).context("invalid hunter ZIP package")?;
    ensure!(archive.len() <= MAX_ENTRIES, "too many package entries");
    let mut entries = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
      let mut entry = archive.by_index(index)?;
      let name = entry.name()?.into_owned();
      let photo = name.strip_prefix("photos/");
      ensure!(
        matches!(name.as_str(), "manifest.json" | "role.bin" | "summary.bin")
          || photo.is_some_and(|suffix| {
            !suffix.contains(['/', '\\']) && is_auxiliary_filename(&format!("SS1_{suffix}"))
          }),
        "unsupported package entry {name}"
      );
      ensure!(
        !entry.is_dir() && entry.unix_mode().is_none_or(|mode| mode & 0o170000 != 0o120000),
        "package entries must be regular files"
      );
      ensure!(entry.size() <= MAX_BYTES - total, "hunter package exceeds safety limits");
      let mut bytes = Vec::new();
      entry.by_ref().take(MAX_BYTES - total + 1).read_to_end(&mut bytes)?;
      total += bytes.len() as u64;
      ensure!(total <= MAX_BYTES, "hunter package exceeds safety limits");
      ensure!(entries.insert(name, bytes).is_none(), "duplicate package entry");
    }
    let manifest: Manifest = serde_json::from_slice(
      &entries.remove("manifest.json").context("missing package manifest")?,
    )?;
    ensure!(
      manifest.format == "mhrise-hunter" && manifest.version == 1,
      "unsupported hunter package version"
    );
    ensure!((1..=3).contains(&manifest.source_slot), "invalid source slot");
    let platform = match manifest.platform.as_str() {
      "steam" if manifest.source_steamid64.is_some() => Platform::Steam,
      "switch" if manifest.source_steamid64.is_none() => Platform::NintendoSwitch,
      _ => bail!("invalid package platform or account metadata"),
    };
    let plain = entries.remove("role.bin").context("missing hunter data")?;
    let role = SavePayload::parse_at_offset(&plain, class_stream_offset(platform))?;
    validate_role_position(&role, manifest.source_slot)?;
    let mut summary =
      SavePayload::parse(&entries.remove("summary.bin").context("missing hunter summary")?)?;
    ensure!(
      summary.entries.len() == 1 && summary.entries[0].native_hash == 0,
      "invalid hunter summary"
    );
    let summary = summary.entries.remove(0).class;
    ensure!(
      empty_summary::compatible(&summary, &empty_summary::builtin(platform)?),
      "unsupported hunter summary schema"
    );
    ensure!(
      scalar_i32(class_field(&summary, HR)?)? >= 0 && scalar_i32(class_field(&summary, MR)?)? >= 0,
      "cannot import an empty hunter"
    );
    string(class_field(&summary, NAME)?)?;
    let id = hunter_id(&role, platform)?;
    ensure!(id.iter().any(|byte| *byte != 0), "empty hunter identity");
    if platform == Platform::Steam {
      ensure!(
        character_id(class_field(&summary, CONSISTENCY)?)? == id,
        "package hunter and summary identities differ"
      );
    }
    let mut photos = BTreeMap::new();
    for (name, bytes) in entries {
      let header = checked_header(&bytes)?;
      ensure!(header.flags.bits() == 0, "package photos must be account-independent");
      // Validate the complete payload now, before any output is created.
      convert_auxiliary_bytes(&bytes, TargetPlatform::NintendoSwitch, None)?;
      photos.insert(name.strip_prefix("photos/").unwrap().to_owned(), bytes);
    }
    Ok(Self { manifest, platform, plain, role, summary, photos })
  }
}

fn hunter_id(role: &SavePayload, platform: Platform) -> Result<&[u8]> {
  character_id(if platform == Platform::Steam {
    field(role, CHARACTER, CONSISTENCY)?
  } else {
    field(role, HUNTER_RECORD, HUNTER_ID)?
  })
}

/// Import a same-platform hunter into a new complete save bundle.
/// Occupied destinations require explicit replacement. Other hunters remain byte-for-byte intact.
pub fn import_hunter(
  package: &Path,
  input: &Path,
  output: &Path,
  slot: u8,
  options: SlotOptions,
  replace: bool,
) -> Result<Vec<PathBuf>> {
  ensure!((1..=3).contains(&slot), "slot must be 1–3");
  let hunter = HunterPackage::read(package)?;
  let source = fs::canonicalize(input)?;
  ensure_output_absent(output)?;
  let parent =
    output.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
  let parent = fs::canonicalize(parent).context("output parent directory must already exist")?;
  ensure!(!parent.starts_with(&source), "output must be outside the source save directory");
  let mut bundle = load_bundle(&source, options)?;
  ensure!(
    bundle.inspection.platform == hunter.platform,
    "hunter packages can only be imported into the same platform"
  );
  ensure!(
    replace || !bundle.roles.contains_key(&slot),
    "destination is occupied; explicitly confirm replacement"
  );
  let id = hunter_id(&hunter.role, hunter.platform)?;
  for (number, (_, role)) in &bundle.roles {
    ensure!(
      *number == slot || hunter_id(role, hunter.platform)? != id,
      "this hunter already exists in slot {number}"
    );
  }
  let offset = class_stream_offset(hunter.platform);
  let mut plain =
    move_role(&hunter.plain, &hunter.role, offset, hunter.manifest.source_slot, slot)?;
  if hunter.platform == Platform::Steam {
    crate::identity::resign_owner_identities(
      &mut plain,
      hunter.manifest.source_steamid64.unwrap(),
      bundle.options.steamid64.unwrap(),
    )?;
  }
  let mut system = bundle.system.clone();
  let FieldValue::Array(summaries) = &mut field_mut(&mut system, LOAD_INFO, HUNTERS)?.value else {
    bail!("hunter summaries are not an array")
  };
  let index = usize::from(slot - 1);
  let ArrayValue::Class(target) = &summaries.values[index] else {
    bail!("invalid destination summary")
  };
  ensure!(
    empty_summary::compatible(&hunter.summary, target),
    "source and destination summary schemas differ"
  );
  if let Some(hashes) = &mut summaries.class_hashes {
    hashes[index] = hunter.summary.hash;
  }
  summaries.values[index] = ArrayValue::Class(Box::new(hunter.summary));
  let system_plain = system.encode_at_offset(offset)?;
  ensure!(
    SavePayload::parse_at_offset(&system_plain, offset)? == system,
    "import changed unrelated system fields"
  );
  bundle.files.insert(SYSTEM_FILE.into(), repack(&system_plain, &bundle)?);
  bundle
    .files
    .retain(|name, _| name != &role_filename(slot) && !name.starts_with(&format!("SS{slot}_")));
  bundle.files.insert(role_filename(slot), repack(&plain, &bundle)?);
  let target = if hunter.platform == Platform::Steam {
    TargetPlatform::Steam
  } else {
    TargetPlatform::NintendoSwitch
  };
  for (suffix, photo) in hunter.photos {
    bundle.files.insert(
      format!("SS{slot}_{suffix}"),
      convert_auxiliary_bytes(&photo, target, bundle.options.steamid64)?,
    );
  }
  write_bundle(&parent, output, bundle.files, bundle.options)
}
