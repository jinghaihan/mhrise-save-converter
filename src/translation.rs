use crate::payload::{Array, ArrayValue, Class, Field, FieldValue, NativeClass, SavePayload};

const SAVE_FILE_DETAIL_NATIVE_HASH: u32 = 0x85e9_04c1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeReport {
  pub source_top_level_classes: usize,
  pub target_top_level_classes: usize,
  pub merged_top_level_classes: usize,
  pub preserved_target_top_level_classes: usize,
  pub copied_source_fields: usize,
  pub preserved_target_fields: usize,
}

pub fn merge_onto_template(
  source: &SavePayload,
  target_template: &SavePayload,
) -> (SavePayload, MergeReport) {
  let mut report = MergeReport {
    source_top_level_classes: source.entries.len(),
    target_top_level_classes: target_template.entries.len(),
    ..MergeReport::default()
  };

  let entries = target_template
    .entries
    .iter()
    .map(|target| merge_top_level(source, target, &mut report))
    .collect();

  (SavePayload { entries }, report)
}

fn merge_top_level(
  source: &SavePayload,
  target: &NativeClass,
  report: &mut MergeReport,
) -> NativeClass {
  if target.native_hash == SAVE_FILE_DETAIL_NATIVE_HASH {
    report.preserved_target_top_level_classes += 1;
    return target.clone();
  }

  let Some(source) = source.entries.iter().find(|source| {
    source.native_hash == target.native_hash && source.class.hash == target.class.hash
  }) else {
    report.preserved_target_top_level_classes += 1;
    return target.clone();
  };

  report.merged_top_level_classes += 1;
  NativeClass {
    native_hash: target.native_hash,
    class: merge_class(&source.class, &target.class, report),
  }
}

fn merge_class(source: &Class, target: &Class, report: &mut MergeReport) -> Class {
  if source.hash != target.hash || source.hash == 0 {
    return source.clone();
  }

  let fields = target
    .fields
    .iter()
    .map(|target_field| {
      let source_field = source.fields.iter().find(|source_field| {
        source_field.hash == target_field.hash && source_field.field_type == target_field.field_type
      });
      match source_field {
        Some(source_field) => {
          report.copied_source_fields += 1;
          Field {
            hash: target_field.hash,
            field_type: target_field.field_type,
            value: merge_value(&source_field.value, &target_field.value, report),
          }
        }
        None => {
          report.preserved_target_fields += 1;
          target_field.clone()
        }
      }
    })
    .collect();

  Class { hash: target.hash, fields }
}

fn merge_value(source: &FieldValue, target: &FieldValue, report: &mut MergeReport) -> FieldValue {
  match (source, target) {
    (FieldValue::Class(source), FieldValue::Class(target)) => {
      FieldValue::Class(Box::new(merge_class(source, target, report)))
    }
    (FieldValue::Array(source), FieldValue::Array(target)) => {
      FieldValue::Array(merge_array(source, target, report))
    }
    _ => source.clone(),
  }
}

fn merge_array(source: &Array, target: &Array, report: &mut MergeReport) -> Array {
  if source.member_type != target.member_type || source.array_type != target.array_type {
    report.preserved_target_fields += 1;
    return target.clone();
  }

  let values = source
    .values
    .iter()
    .enumerate()
    .map(|(index, source_value)| match source_value {
      ArrayValue::Class(source_class) => {
        let target_class = target
          .values
          .get(index)
          .and_then(as_class)
          .filter(|class| class.hash == source_class.hash)
          .or_else(|| {
            target.values.iter().filter_map(as_class).find(|class| class.hash == source_class.hash)
          });
        match target_class {
          Some(target_class) => {
            ArrayValue::Class(Box::new(merge_class(source_class, target_class, report)))
          }
          None => source_value.clone(),
        }
      }
      _ => source_value.clone(),
    })
    .collect();

  Array {
    member_type: source.member_type,
    member_size: source.member_size,
    array_type: source.array_type,
    class_hashes: source.class_hashes.clone(),
    values,
  }
}

fn as_class(value: &ArrayValue) -> Option<&Class> {
  match value {
    ArrayValue::Class(class) => Some(class),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn scalar(value: u32) -> FieldValue {
    FieldValue::Scalar { size: 4, bytes: value.to_le_bytes().to_vec() }
  }

  fn field(hash: u32, value: u32) -> Field {
    Field { hash, field_type: 8, value: scalar(value) }
  }

  #[test]
  fn follows_target_schema_and_copies_matching_source_values() {
    let source = SavePayload {
      entries: vec![NativeClass {
        native_hash: 1,
        class: Class { hash: 10, fields: vec![field(100, 1), field(101, 2)] },
      }],
    };
    let target = SavePayload {
      entries: vec![
        NativeClass {
          native_hash: 1,
          class: Class { hash: 10, fields: vec![field(100, 9), field(102, 3)] },
        },
        NativeClass { native_hash: 2, class: Class { hash: 20, fields: vec![field(200, 4)] } },
      ],
    };

    let (merged, report) = merge_onto_template(&source, &target);

    assert_eq!(merged.entries.len(), 2);
    assert_eq!(merged.entries[0].class.fields, vec![field(100, 1), field(102, 3)]);
    assert_eq!(merged.entries[1], target.entries[1]);
    assert_eq!(report.merged_top_level_classes, 1);
    assert_eq!(report.preserved_target_top_level_classes, 1);
    assert_eq!(report.copied_source_fields, 1);
    assert_eq!(report.preserved_target_fields, 1);
  }

  #[test]
  fn preserves_target_file_detail_metadata() {
    let source = SavePayload {
      entries: vec![NativeClass {
        native_hash: SAVE_FILE_DETAIL_NATIVE_HASH,
        class: Class { hash: 10, fields: vec![field(100, 1)] },
      }],
    };
    let target = SavePayload {
      entries: vec![NativeClass {
        native_hash: SAVE_FILE_DETAIL_NATIVE_HASH,
        class: Class { hash: 10, fields: vec![field(100, 9)] },
      }],
    };

    let (merged, report) = merge_onto_template(&source, &target);

    assert_eq!(merged, target);
    assert_eq!(report.preserved_target_top_level_classes, 1);
    assert_eq!(report.copied_source_fields, 0);
  }
}
