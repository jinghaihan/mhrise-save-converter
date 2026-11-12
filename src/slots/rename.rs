use super::*;
use crate::payload::{Class, encode_field_at_offset};

// GuildCardSaveData.MyData.HunterName, verified against the game's type schema.
// Historical arena records and other hunters' guild cards deliberately stay unchanged.
const GUILD_CARD: u32 = 0x8c6f_b4c6;
const MY_DATA: u32 = 0xd51b_2c9f;
const HUNTER_NAME: u32 = 0x02bb_0110;

pub(super) fn validate_name(name: &str) -> Result<()> {
  ensure!(!name.trim().is_empty(), "hunter name cannot be blank");
  ensure!(!name.chars().any(char::is_control), "hunter name cannot contain control characters");
  ensure!(name.encode_utf16().count() <= 12, "hunter name must fit in 12 UTF-16 characters");
  Ok(())
}

pub(super) fn set_summary_name(summary: &mut Class, name: &str) -> Result<()> {
  string(class_field(summary, NAME)?)?;
  summary.fields.iter_mut().find(|field| field.hash == NAME).unwrap().value =
    FieldValue::String(name.encode_utf16().collect());
  Ok(())
}

pub(super) fn rename_role(
  plain: &[u8],
  role: &SavePayload,
  offset: usize,
  old_name: &str,
  name: &str,
) -> Result<(Vec<u8>, SavePayload)> {
  let mut expected = role.clone();
  let FieldValue::Class(owned) = &mut field_mut(&mut expected, GUILD_CARD, MY_DATA)?.value else {
    bail!("unsupported owner guild-card layout")
  };
  ensure!(
    string(class_field(owned, HUNTER_NAME)?)? == old_name,
    "hunter summary and owner guild-card names differ; refusing ambiguous rename"
  );
  let field = owned.fields.iter_mut().find(|field| field.hash == HUNTER_NAME).unwrap();
  field.value = FieldValue::String(name.encode_utf16().collect());
  let paths: &[&[u32]] = &[&[GUILD_CARD, MY_DATA, HUNTER_NAME]];
  let ranges = field_ranges(plain, offset, paths)?;
  ensure!(ranges.len() == 1, "missing or duplicate owner name");
  let range = &ranges[0].1;
  let replacement = encode_field_at_offset(field, offset + range.start)?;
  let mut patched = plain[..range.start].to_vec();
  patched.extend_from_slice(&replacement);
  patched.extend_from_slice(&plain[range.end..]);
  if !SavePayload::parse_at_offset(&patched, offset).is_ok_and(|parsed| parsed == expected) {
    patched = expected.encode_at_offset(offset)?;
  }
  ensure!(
    SavePayload::parse_at_offset(&patched, offset)? == expected,
    "rename changed unrelated character fields"
  );
  Ok((patched, expected))
}
