# fnt4-tool

FNT4 font extract/repack/rebuild tool. Ported from [konosuba_py](https://github.com/lzhhzl/about-shin/tree/main/konosuba_py).

Only tested on *AstralAir no Shiroki Towa -White Eternity-* (FNT4 V1) and *Irotoridori no Sekai WORLD'S END -RE:BIRTH-* (FNT4 V0).

## Usage

### Extract

```bash
fnt4-tool extract input.fnt output_dir
```

### Repack

```bash
fnt4-tool repack input_dir output.fnt
```

### Rebuild

```bash
fnt4-tool rebuild input.fnt output.fnt source_font.ttf -q 4

```

#### Rebuild options

- `-s`/`--size`: Font size in pixels. If not specified, auto-calculated from original FNT (ascent + descent)
- `-q`/`--quality`: Quality factor. Renders at higher resolution then downsamples with Lanczos filter. Higher = cleaner edges but slower. Recommended: 2-4. Default: 1 (no supersampling)
- `--letter-spacing`: Letter spacing pixels. Default: 0
- `--texture-padding`: Texture padding pixels. If not specified, auto-calculated from original FNT (mipmap level)
- `-c`/`--config`: Rebuild config from a toml file. See [config.toml](examples/config.toml) for an example.

##### Glyph replacement

If the character you're using isn't in the fnt, you can specify a `[replace]` section in the [config.toml](examples/config.toml) to `replace` characters in the FNT4 font to different characters in the source TTF/OTF font for glyph replacement.

If you are using [shin-translation-tools](https://github.com/DCNick3/shin-translation-tools), you can use the [create-mapping.py](examples/create-mapping.py) script to automatically generate a mapping toml file and a new mapped CSV file from the CSV file.

## Build

```bash
cargo build --release
```

**Note:** Always use `--release` for better performance.

## Test

```bash
cargo test
```

## Thanks

Thanks to the following projects for their work:

- [konosuba_py](https://github.com/lzhhzl/about-shin/tree/main/konosuba_py)
- [shin-translation-tools](https://github.com/DCNick3/shin-translation-tools)
