> [!WARNING]
> **WIP** — Experimental and not yet production-ready. Always back up your saves before use.

# MHRise Save Converter

Save-container tooling and experimental cross-platform conversion research for *Monster Hunter Rise*, targeting Nintendo Switch and Steam saves.

## Status

Steam-to-Steam account resigning is implemented at the DSSS/Citrus container layer. Experimental cross-platform schema translation is implemented as well: a destination save can be used as a schema/default template, and Switch-to-Steam conversion can fall back to built-in Steam defaults. In-game validation across more saves and game versions is still pending, so all conversion paths remain WIP.

## Save Structure

Monster Hunter Rise saves are directories containing several DSSS container files. The converter handles the core gameplay files and the known album/photo containers:

| File or pattern | Contents | Converter behavior |
| --- | --- | --- |
| `data00-1.bin` | System and global save data, such as settings and account-level state | Container and experimental platform-schema conversion |
| `data###Slot.bin` | Character/save-slot data, such as hunter progress, equipment, items, and quests | Container and experimental platform-schema conversion |
| `SS1_*`, `SS4_*`, `SS7_*` | Screenshot and album-related auxiliary data | Wrapper conversion; the contained album payload is preserved |
| Other files | Version- or feature-specific auxiliary files | Ignored unless explicitly supported |

The two core file types use related logical payloads and different platform containers:

- Nintendo Switch uses DSSS v2 with the `DEFLATE` flag (`0x08`). The payload is raw DEFLATE-compressed.
- Steam uses DSSS v2 with the `CITRUS` flag (`0x04`). The payload is protected by SteamID64-dependent AES/ECC encryption and a Citrus Curve Index.
- Known auxiliary files use an unencrypted wrapper: Switch uses no extra flag, while Steam uses `HAS_ID` (`0x02`) and stores the account identifier in the wrapper.
- Both formats carry an outer MurmurHash3 integrity value. Steam files also contain per-block Citrus integrity checks.

Same-platform resigning decrypts the core payload, keeps it unchanged, then repacks it and regenerates the required integrity values. Cross-platform conversion additionally parses the self-describing RE Engine class stream, follows the destination class order, carries over matching source fields, and preserves or constructs destination-only classes and fields. Known auxiliary files are rewrapped while preserving their filenames, slot numbers, and payload bytes. Files absent from the source directory cannot be reconstructed; keep the original save directory as a backup.

## Usage

```bash
cargo run -- inspect /path/to/win64_save
cargo run -- verify /path/to/win64_save
```

The inspector identifies core save files, platform flags, and file-integrity checks without modifying the input. The verifier exits with a failure status if any core file has an invalid outer checksum.

### Switch → Steam

Using an existing Steam save as the target template is preferred. It supplies the exact destination schema, platform settings, key configuration, save metadata, and Curve Index:

```bash
cargo run -- convert /path/to/monster-hunter-rise-ns /tmp/mhrise-steam \
  --to steam \
  --target-steamid64 76561198382766028 \
  --target-reference /path/to/existing/win64_save
```

When no Steam template is available, provide the target Curve Index explicitly. The converter constructs the known Steam-only classes and fields from built-in defaults:

```bash
cargo run -- convert /path/to/monster-hunter-rise-ns /tmp/mhrise-steam \
  --to steam \
  --target-steamid64 76561198382766028 \
  --target-curve-index 116
```

### Steam → Switch

Steam-to-Switch currently requires a Switch target template because several Switch-only DLC fields have no trustworthy source on Steam:

```bash
cargo run -- convert /path/to/win64_save /tmp/mhrise-switch \
  --to switch \
  --source-steamid64 76561198382766028 \
  --source-curve-index 116 \
  --target-reference /path/to/existing/switch-save
```

If the source Curve Index is unknown, omit `--source-curve-index` and the tool will try to detect it. For a Steam destination, `--target-reference` also detects the target Curve Index; without a reference, `--target-curve-index` is required. A reference directory should contain matching `data00-1.bin` and slot filenames.

The converter rewrites the core `data00-1.bin` and `data###Slot.bin` files together with known `SS1_*`, `SS4_*`, and `SS7_*` files. Other files are intentionally left out. A destination template is read-only and is never modified. Use a new output directory and keep both original saves as backups; `--force` is required to reuse a non-empty output path.

> [!IMPORTANT]
> Successful parsing and checksum verification only prove structural validity. The game is the final compatibility test; disable Steam Cloud and test with backed-up local saves first.

### Graphical interface

Launch the native GUI with:

```bash
cargo run --release --bin mhrise-save-gui
```

Choose the source and a new output directory, select the target platform, and optionally choose a target save as the schema/template reference. The GUI runs the same preflight checks as the CLI, converts in a background worker, reports per-file progress, and can open the completed output directory. The source and template directories are never modified.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Rust formatting uses two-space indentation as configured in `rustfmt.toml`. Every push and pull request runs formatting, Clippy, tests, and release builds for Linux, macOS, and Windows. The build workflow uploads platform archives and SHA-256 files as workflow artifacts.

To publish a release from a clean, up-to-date `main` branch, use the release helper. The `--current` form is useful when the version in `Cargo.toml` is already the intended version:

```bash
python3 tools/release.py --current --yes   # publishes v0.1.0 for this release
python3 tools/release.py --bump patch      # interactively publishes the next patch release
```

Pushing a `v*` tag starts the release workflow, which rebuilds all three platforms, verifies SHA-256 checksums, and creates the GitHub Release with the archives attached.

## Credits

Format research and implementation references:

- [kvasszn/ree-save-editor](https://github.com/kvasszn/ree-save-editor), especially its MH Rise DSSS/Citrus research and account-transfer notes.

This project is an independent implementation and does not include the referenced project's GUI or source tree.

## License

MIT. See [LICENSE](LICENSE).
