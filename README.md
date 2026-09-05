> [!WARNING]
> **WIP** — Experimental and not yet production-ready. Always back up your saves before use.

# MHRise Save Converter

Cross-platform save conversion and Steam-account resigning for *Monster Hunter Rise*, targeting Nintendo Switch and Steam saves.

## Status

The project is in early development. The current CLI milestone focuses on inspecting DSSS save containers safely; conversion support will be added incrementally and will always write to a separate output directory.

## Usage

```bash
cargo run -- inspect /path/to/win64_save
```

The inspector identifies core save files, platform flags, and file-integrity checks without modifying the input.

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
- Preserve save data while regenerating platform-specific integrity checks.
