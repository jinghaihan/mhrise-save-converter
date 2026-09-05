> [!WARNING]
> **WIP** — Experimental and not yet production-ready. Always back up your saves before use.

# MHRise Save Converter

Cross-platform save conversion and Steam-account resigning for *Monster Hunter Rise*, targeting Nintendo Switch and Steam saves.

## Status

The CLI can inspect, verify, and convert the two core save files between Nintendo Switch and Steam. Steam conversion supports account resigning when you provide the destination SteamID64 and Curve Index.

## Save Structure

Monster Hunter Rise saves are directories containing several DSSS container files. The converter currently handles the gameplay save files only:

| File or pattern | Contents | Converter behavior |
| --- | --- | --- |
| `data00-1.bin` | System and global save data, such as settings and account-level state | Converted |
| `data###Slot.bin` | Character/save-slot data, such as hunter progress, equipment, items, and quests | Converted |
| `SS1_*`, `SS4_*`, `SS7_*` | Screenshot and album-related auxiliary data | Ignored |
| Other files | Version- or feature-specific auxiliary files | Ignored unless explicitly supported |

The two core file types have the same logical payload but different platform containers:

- Nintendo Switch uses DSSS v2 with the `DEFLATE` flag (`0x08`). The payload is raw DEFLATE-compressed.
- Steam uses DSSS v2 with the `CITRUS` flag (`0x04`). The payload is protected by SteamID64-dependent AES/ECC encryption and a Citrus Curve Index.
- Both formats carry an outer MurmurHash3 integrity value. Steam files also contain per-block Citrus integrity checks.

Conversion decompresses or decrypts the core payload, keeps that payload unchanged, then repacks it into the target platform container and regenerates the required integrity values. Steam account transfer changes the Citrus encryption identity; it does not convert the screenshot or album files. If those auxiliary files are needed, copy them separately and keep the original save directory as a backup.

## Usage

```bash
cargo run -- inspect /path/to/win64_save
cargo run -- verify /path/to/win64_save
```

The inspector identifies core save files, platform flags, and file-integrity checks without modifying the input. The verifier exits with a failure status if any core file has an invalid outer checksum.

### Switch → Steam

```bash
cargo run -- convert /path/to/monster-hunter-rise-ns /tmp/mhrise-steam \
  --to steam \
  --target-steamid64 76561198382766028 \
  --target-curve-index 116
```

### Steam → Switch

```bash
cargo run -- convert /path/to/win64_save /tmp/mhrise-switch \
  --to switch \
  --source-steamid64 76561198382766028 \
  --source-curve-index 116
```

If the source Curve Index is unknown, omit `--source-curve-index` and the tool will try to detect it. To use the Curve Index from an existing target Steam save, provide `--target-reference` together with `--target-steamid64`:

```bash
cargo run -- convert /path/to/monster-hunter-rise-ns /tmp/mhrise-steam \
  --to steam \
  --target-steamid64 76561198382766028 \
  --target-reference /path/to/existing/win64_save
```

The converter only rewrites the core `data00-1.bin` and `data###Slot.bin` files. Album and screenshot files such as `SS1_*`, `SS4_*`, and `SS7_*` are not part of the cross-platform save payload and are intentionally ignored. Use a new output directory and keep the original save as a backup; `--force` is required to reuse a non-empty output path.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Credits

Format research and implementation references:

- [kvasszn/ree-save-editor](https://github.com/kvasszn/ree-save-editor), especially its MH Rise DSSS/Citrus research and account-transfer notes.

This project is an independent implementation and does not include the referenced project's GUI or source tree.

## License

MIT. See [LICENSE](LICENSE).
