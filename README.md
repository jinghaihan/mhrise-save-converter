# MHRise Save Converter

Native GUI and library tooling for converting *Monster Hunter Rise* saves between Nintendo Switch and Steam formats.

> [!WARNING]
> **Test status:** Nintendo Switch → Steam conversion has been tested and confirmed working. Steam → Nintendo Switch and Steam → Steam have not been tested yet. Always back up your saves first.

## GUI

Download the latest package from [Releases](https://github.com/jinghaihan/mhrise-save-converter/releases), or run it from source:

```bash
cargo run --release --bin mhrise-save-converter-gui
```

Choose a source save, a new output directory, and the target platform. For Steam conversion, enter the relevant SteamID64 and preferably select an existing target save as the template; the app can read the destination Curve Index automatically. The source and template are never modified. The GUI performs a preflight check, converts in the background, shows per-file progress, and can open the output folder.

## Steam inputs

SteamID64 is the account's 17-digit numeric Steam identifier, not a display name or custom profile name. A numeric Steam profile URL has the form `steamcommunity.com/profiles/<STEAMID64>`. For details about SteamID64, Curve Index, save locations, and the advanced CLI, see [docs/cli.md](docs/cli.md).

The supported file layout and container details are documented in [docs/save-structure.md](docs/save-structure.md).

## Credits

Format research and implementation references:

- [kvasszn/ree-save-editor](https://github.com/kvasszn/ree-save-editor), especially its MH Rise DSSS/Citrus research and account-transfer notes.

This project is an independent implementation and does not include the referenced project's GUI or source tree.

## License

[MIT](./LICENSE) License © [jinghaihan](https://github.com/jinghaihan)
