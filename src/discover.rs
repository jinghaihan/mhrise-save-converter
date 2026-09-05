use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::format::{Platform, checksum_status, inspect_path};

#[derive(Debug, Clone)]
pub struct CoreFile {
    pub path: PathBuf,
    pub platform: Platform,
    pub checksum: String,
}

pub fn is_core_filename(name: &str) -> bool {
    if name == "data00-1.bin" {
        return true;
    }

    let Some(slot_number) =
        name.strip_prefix("data").and_then(|value| value.strip_suffix("Slot.bin"))
    else {
        return false;
    };
    slot_number.len() == 3 && slot_number.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn discover_core_files(input: &Path) -> Result<Vec<CoreFile>> {
    let paths = if input.is_file() {
        vec![input.to_path_buf()]
    } else {
        let mut paths = fs::read_dir(input)
            .with_context(|| format!("could not read save directory {}", input.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.retain(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(is_core_filename)
        });
        paths.sort();
        paths
    };

    if paths.is_empty() {
        anyhow::bail!("no core save files found in {}", input.display());
    }

    paths
        .into_iter()
        .map(|path| {
            let (header, _) = inspect_path(&path)
                .with_context(|| format!("could not inspect {}", path.display()))?;
            let data = fs::read(&path)?;
            let checksum = checksum_status(&data)?.to_string();
            Ok(CoreFile { path, platform: header.platform(), checksum })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::is_core_filename;

    #[test]
    fn finds_core_files_but_ignores_album_files() {
        assert!(is_core_filename("data00-1.bin"));
        assert!(is_core_filename("data001Slot.bin"));
        assert!(is_core_filename("data003Slot.bin"));
        assert!(!is_core_filename("SS1_data001Slot.bin"));
        assert!(!is_core_filename("data01Slot.bin"));
        assert!(!is_core_filename("data001Slot.dat"));
    }
}
