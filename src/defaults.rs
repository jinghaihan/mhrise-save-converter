use anyhow::{Result, bail};

use crate::payload::{Array, ArrayValue, Class, Field, FieldValue, NativeClass, SavePayload};

const ARRAY: i32 = -1;
const BOOL: i32 = 0x02;
const U8: i32 = 0x04;
const S32: i32 = 0x07;
const U32: i32 = 0x08;
const S64: i32 = 0x09;
const STRUCT: i32 = 0x10;
const CLASS: i32 = 0x11;
const SYSTEM_MARKER: u32 = 0xe2a3_ea98;
const SLOT_MARKER: u32 = 0xd6f4_726d;
const SAVE_FILE_DETAIL: u32 = 0x85e9_04c1;

const STEAM_SYSTEM_ORDER: &[u32] = &[
  0xe2a3_ea98,
  0xb0ca_70c9,
  0x913e_cbe8,
  0x649e_38ff,
  0x0b19_f75b,
  0x4d64_87d4,
  0x5a04_dbf9,
  0x8195_71e6,
  SAVE_FILE_DETAIL,
];

const STEAM_SLOT_ORDER: &[u32] = &[
  0xd6f4_726d,
  0x1322_883a,
  0x9ccc_3b1e,
  0xadc0_75b6,
  0x20d9_167c,
  0x9381_9625,
  0x8512_ab74,
  0x553d_33b8,
  0x6834_4f29,
  0xb212_9149,
  0xb0ca_70c9,
  0xa812_38c6,
  0xbb43_1b76,
  0x1e32_797d,
  0x355c_8c4f,
  0x8c6f_b4c6,
  0xe58f_b6c9,
  0x1f61_3294,
  0xf2fb_669a,
  0xe825_861b,
  0x356f_270b,
  0xef78_287f,
  0xf5e0_18a4,
  0xcf6c_5091,
  0x81fc_c8f4,
  0xab10_9098,
  0xdc00_f45c,
  0xddb8_e034,
  0x4423_bc21,
  0x5a04_dbf9,
  0x164d_2e71,
  0x51dc_d6fb,
  0xb9c1_5dcf,
  0xaaca_38e3,
  0x22a8_b022,
  0x3da5_b9de,
  0xa21e_01ad,
  0x0386_f39d,
  0xd5f9_1c48,
  SAVE_FILE_DETAIL,
];

pub fn steam_template_from_source(source: &SavePayload) -> Result<SavePayload> {
  let mut target = source.clone();
  if source.entries.iter().any(|entry| entry.native_hash == SYSTEM_MARKER) {
    patch_system(&mut target);
  } else if source.entries.iter().any(|entry| entry.native_hash == SLOT_MARKER) {
    patch_slot(&mut target);
  } else {
    bail!("save payload is neither a recognized system save nor a slot save");
  }
  Ok(target)
}

fn patch_system(payload: &mut SavePayload) {
  patch_load_info(payload);
  patch_dlc_system(payload);
  upsert(payload, steam_version_checker());
  upsert(payload, steam_system_options());
  upsert(payload, steam_key_config());
  reorder(payload, STEAM_SYSTEM_ORDER);
}

fn patch_slot(payload: &mut SavePayload) {
  patch_character_consistency_id(payload);
  patch_pc_telemetry(payload);
  upsert(payload, external_quest_data());
  upsert(payload, steam_character_options());
  reorder(payload, STEAM_SLOT_ORDER);
}

fn patch_load_info(payload: &mut SavePayload) {
  let Some(class) = top_class_mut(payload, 0x0b19_f75b, 0xe6a6_9e8a) else { return };
  insert_before_version(class, array_field(0xbba2_0186, U8, 1, vec![vec![0]; 16]));
}

fn patch_dlc_system(payload: &mut SavePayload) {
  let Some(class) = top_class_mut(payload, 0x4d64_87d4, 0xa0a6_441d) else { return };
  let available = class
    .fields
    .iter()
    .find(|field| field.hash == 0x3811_b01d)
    .cloned()
    .map(|mut field| {
      field.hash = 0x0a50_d2dc;
      field
    })
    .unwrap_or_else(empty_dlc_flags);
  let mut fields = [0x1fc1_b1b3, 0x3811_b01d, 0x4f12_7348, 0xef10_b158]
    .iter()
    .filter_map(|hash| class.fields.iter().find(|field| field.hash == *hash).cloned())
    .collect::<Vec<_>>();
  let version = fields.pop();
  fields.insert(2, available);
  if let Some(version) = version {
    fields.push(version);
  }
  class.fields = fields;
}

fn patch_character_consistency_id(payload: &mut SavePayload) {
  let hunter_id = top_class(payload, 0x355c_8c4f, 0xaf6e_a643)
    .and_then(|class| class.fields.iter().find(|field| field.hash == 0x0a96_0102))
    .and_then(scalar_bytes)
    .filter(|bytes| bytes.len() == 16)
    .map(ToOwned::to_owned)
    .unwrap_or_else(|| vec![0; 16]);
  let Some(class) = top_class_mut(payload, 0x356f_270b, 0x5766_f30b) else { return };
  insert_before_version(class, scalar_field(0xe40f_c0cd, STRUCT, hunter_id));
}

fn patch_pc_telemetry(payload: &mut SavePayload) {
  let Some(snapshot) = top_class_mut(payload, 0x1322_883a, 0x8b6c_3ddc) else { return };
  let Some(product) =
    snapshot.fields.iter_mut().find(|field| field.hash == 0x0d01_1e4c).and_then(|field| {
      match &mut field.value {
        FieldValue::Class(class) if class.hash == 0x9532_0424 => Some(class.as_mut()),
        _ => None,
      }
    })
  else {
    return;
  };
  for hash in [0xff28_f5f3, 0x85b7_7a5a, 0x503b_7ec1, 0x698a_e462] {
    push_if_missing(product, empty_class_array_field(hash));
  }
  push_if_missing(product, scalar_field(0x6e23_2145, S64, 0i64.to_le_bytes().to_vec()));
  for hash in [0x0a36_0ef6, 0xcb7a_2960, 0x4627_d929] {
    push_if_missing(product, empty_class_array_field(hash));
  }
  push_if_missing(product, scalar_field(0x640a_f498, S64, 0i64.to_le_bytes().to_vec()));
}

fn steam_version_checker() -> NativeClass {
  native(0x649e_38ff, 0xf3a7_e6a4, vec![u32_field(0xef10_b158, 827_392_000)])
}

fn steam_system_options() -> NativeClass {
  native(
    0x5a04_dbf9,
    0x609f_5577,
    vec![
      array_field(0x7b7a_a988, S32, 4, vec![vec![0; 4]; 5]),
      s32_field(0x66d8_c850, 50),
      s32_field(0x25c4_fa4e, 0),
      s32_field(0x6546_cbdd, 50),
      s32_field(0x7e68_451a, 0),
      bool_field(0x7216_682b, true),
      bool_field(0xafa1_edf9, false),
      u32_field(0xef10_b158, 1001),
    ],
  )
}

fn steam_key_config() -> NativeClass {
  let user_config = Class {
    hash: 0x1d7e_2ded,
    fields: vec![
      class_field(
        0x4d37_52f4,
        Class { hash: 0x0d2b_2d0d, fields: vec![empty_class_array_field(0x4d37_52f4)] },
      ),
      class_field(
        0x73a0_7131,
        Class {
          hash: 0x33ad_5ad6,
          fields: vec![string_field(0xbcf6_bc33, "Default"), empty_class_array_field(0x7680_621f)],
        },
      ),
    ],
  };
  native(
    0x8195_71e6,
    0xd633_1b34,
    vec![
      class_field(0x049b_afc9, user_config),
      class_field(
        0x6482_2a5f,
        Class {
          hash: 0xbf55_0512,
          fields: vec![
            empty_class_array_field(0xe666_5f54),
            empty_class_array_field(0x912d_f7a2),
            empty_class_array_field(0x73a0_7131),
          ],
        },
      ),
      class_field(
        0x953d_1f7b,
        Class { hash: 0x10c1_11cc, fields: vec![empty_class_array_field(0xb998_da40)] },
      ),
      u32_field(0xef10_b158, 1004),
    ],
  )
}

fn external_quest_data() -> NativeClass {
  native(
    0xbb43_1b76,
    0xef94_546d,
    vec![
      array_field(0x014d_9d98, BOOL, 1, vec![vec![0]; 100]),
      array_field(0x42ff_a9f2, BOOL, 1, vec![vec![0]; 100]),
      array_field(0x26bc_7c4a, BOOL, 1, vec![vec![0]; 100]),
      array_field(0xba5c_3e3f, S64, 8, vec![vec![0; 8]; 100]),
      u32_field(0xef10_b158, 1),
    ],
  )
}

fn steam_character_options() -> NativeClass {
  native(
    0x5a04_dbf9,
    0xd71a_55bd,
    vec![
      array_field(0xf4f5_060e, S32, 4, vec![vec![0; 4]; 2]),
      array_field(0xe33a_ee58, S32, 4, vec![vec![0; 4]; 11]),
      array_field(0x4371_30de, S32, 4, vec![vec![0; 4]; 7]),
      u32_field(0xef10_b158, 1000),
    ],
  )
}

fn empty_dlc_flags() -> Field {
  class_field(
    0x0a50_d2dc,
    Class { hash: 0xfdb0_c267, fields: vec![array_field(0x6829_aab6, U32, 4, Vec::new())] },
  )
}

fn native(native_hash: u32, class_hash: u32, fields: Vec<Field>) -> NativeClass {
  NativeClass { native_hash, class: Class { hash: class_hash, fields } }
}

fn scalar_field(hash: u32, field_type: i32, bytes: Vec<u8>) -> Field {
  Field { hash, field_type, value: FieldValue::Scalar { size: bytes.len() as u32, bytes } }
}

fn scalar_bytes(field: &Field) -> Option<&[u8]> {
  match &field.value {
    FieldValue::Scalar { bytes, .. } => Some(bytes),
    _ => None,
  }
}

fn s32_field(hash: u32, value: i32) -> Field {
  scalar_field(hash, S32, value.to_le_bytes().to_vec())
}

fn u32_field(hash: u32, value: u32) -> Field {
  scalar_field(hash, U32, value.to_le_bytes().to_vec())
}

fn bool_field(hash: u32, value: bool) -> Field {
  scalar_field(hash, BOOL, vec![u8::from(value)])
}

fn string_field(hash: u32, value: &str) -> Field {
  Field { hash, field_type: 0x0f, value: FieldValue::String(value.encode_utf16().collect()) }
}

fn class_field(hash: u32, class: Class) -> Field {
  Field { hash, field_type: CLASS, value: FieldValue::Class(Box::new(class)) }
}

fn array_field(hash: u32, member_type: i32, member_size: u32, values: Vec<Vec<u8>>) -> Field {
  Field {
    hash,
    field_type: ARRAY,
    value: FieldValue::Array(Array {
      member_type,
      member_size,
      array_type: 0,
      class_hashes: None,
      values: values.into_iter().map(ArrayValue::Scalar).collect(),
    }),
  }
}

fn empty_class_array_field(hash: u32) -> Field {
  Field {
    hash,
    field_type: ARRAY,
    value: FieldValue::Array(Array {
      member_type: CLASS,
      member_size: 8,
      array_type: 1,
      class_hashes: None,
      values: Vec::new(),
    }),
  }
}

fn top_class(payload: &SavePayload, native_hash: u32, class_hash: u32) -> Option<&Class> {
  payload
    .entries
    .iter()
    .find(|entry| entry.native_hash == native_hash && entry.class.hash == class_hash)
    .map(|entry| &entry.class)
}

fn top_class_mut(
  payload: &mut SavePayload,
  native_hash: u32,
  class_hash: u32,
) -> Option<&mut Class> {
  payload
    .entries
    .iter_mut()
    .find(|entry| entry.native_hash == native_hash && entry.class.hash == class_hash)
    .map(|entry| &mut entry.class)
}

fn insert_before_version(class: &mut Class, field: Field) {
  if class.fields.iter().any(|current| current.hash == field.hash) {
    return;
  }
  let index = class
    .fields
    .iter()
    .position(|current| current.hash == 0xef10_b158)
    .unwrap_or(class.fields.len());
  class.fields.insert(index, field);
}

fn push_if_missing(class: &mut Class, field: Field) {
  if !class.fields.iter().any(|current| current.hash == field.hash) {
    class.fields.push(field);
  }
}

fn upsert(payload: &mut SavePayload, entry: NativeClass) {
  if let Some(existing) =
    payload.entries.iter_mut().find(|existing| existing.native_hash == entry.native_hash)
  {
    *existing = entry;
  } else {
    payload.entries.push(entry);
  }
}

fn reorder(payload: &mut SavePayload, order: &[u32]) {
  let mut remaining = std::mem::take(&mut payload.entries);
  let mut ordered = Vec::with_capacity(remaining.len());
  for native_hash in order {
    if let Some(index) = remaining.iter().position(|entry| entry.native_hash == *native_hash) {
      ordered.push(remaining.remove(index));
    }
  }
  if let Some(index) = ordered.iter().position(|entry| entry.native_hash == SAVE_FILE_DETAIL) {
    let detail = ordered.remove(index);
    ordered.append(&mut remaining);
    ordered.push(detail);
  } else {
    ordered.append(&mut remaining);
  }
  payload.entries = ordered;
}

#[cfg(test)]
mod tests {
  use super::*;

  fn empty_entry(native_hash: u32, class_hash: u32) -> NativeClass {
    native(native_hash, class_hash, Vec::new())
  }

  #[test]
  fn builds_system_profile_with_steam_only_classes() {
    let mut source = SavePayload {
      entries: vec![
        empty_entry(SYSTEM_MARKER, 1),
        native(0x0b19_f75b, 0xe6a6_9e8a, vec![u32_field(0xef10_b158, 7)]),
        empty_entry(0x4d64_87d4, 0xa0a6_441d),
        empty_entry(SAVE_FILE_DETAIL, 2),
      ],
    };
    source.entries[2].class.fields = vec![
      s32_field(0x1fc1_b1b3, 5),
      empty_dlc_flags(),
      s32_field(0x4f12_7348, 1),
      u32_field(0xef10_b158, 5),
    ];
    source.entries[2].class.fields[1].hash = 0x3811_b01d;

    let target = steam_template_from_source(&source).expect("system profile should translate");

    assert_eq!(target.entries.last().map(|entry| entry.native_hash), Some(SAVE_FILE_DETAIL));
    assert!(target.entries.iter().any(|entry| entry.native_hash == 0x649e_38ff));
    assert!(target.entries.iter().any(|entry| entry.native_hash == 0x8195_71e6));
    let load_info = top_class(&target, 0x0b19_f75b, 0xe6a6_9e8a).unwrap();
    assert_eq!(
      load_info.fields.iter().map(|field| field.hash).collect::<Vec<_>>(),
      vec![0xbba2_0186, 0xef10_b158]
    );
  }

  #[test]
  fn builds_slot_profile_and_reuses_hunter_guid() {
    let guid = (0u8..16).collect::<Vec<_>>();
    let source = SavePayload {
      entries: vec![
        empty_entry(SLOT_MARKER, 1),
        native(0x355c_8c4f, 0xaf6e_a643, vec![scalar_field(0x0a96_0102, STRUCT, guid.clone())]),
        native(0x356f_270b, 0x5766_f30b, vec![u32_field(0xef10_b158, 1)]),
        empty_entry(SAVE_FILE_DETAIL, 2),
      ],
    };

    let target = steam_template_from_source(&source).expect("slot profile should translate");

    assert!(target.entries.iter().any(|entry| entry.native_hash == 0xbb43_1b76));
    assert!(target.entries.iter().any(|entry| entry.native_hash == 0x5a04_dbf9));
    let system = top_class(&target, 0x356f_270b, 0x5766_f30b).unwrap();
    let consistency = system.fields.iter().find(|field| field.hash == 0xe40f_c0cd).unwrap();
    assert_eq!(scalar_bytes(consistency), Some(guid.as_slice()));
  }

  #[test]
  fn rejects_unknown_payload_profile() {
    let error = steam_template_from_source(&SavePayload { entries: Vec::new() })
      .expect_err("unknown payload should fail");
    assert!(error.to_string().contains("neither a recognized system save nor a slot save"));
  }
}
