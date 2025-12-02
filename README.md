# fnt4-tool

FNT4 font extract/repack/rebuild tool. Ported from [konosuba_py](https://github.com/lzhhzl/about-shin/tree/main/konosuba_py).

Only tested on *AstralAir no Shiroki Towa -White Eternity-*.

## Usage

### Extract

```bash
fnt4-tool extract input.fnt output_dir
```

### Repack (FNT4 V1 only)

```bash
fnt4-tool repack input_dir output.fnt
```

### Rebuild (FNT4 V1 only)

```bash
fnt4-tool rebuild input.fnt output.fnt source_font.ttf -q 4

```

#### Rebuild options

- `-s`/`--size`: Font size in pixels. If not specified, auto-calculated from original FNT (ascent + descent)
- `-p`/`--padding`: Font padding pixels. Default: 4
- `-q`/`--quality`: Quality factor (1-8). Renders at higher resolution then downsamples with Lanczos filter. Higher = cleaner edges but slower. Recommended: 2-4. Default: 1 (no supersampling)
- `-c`/`--config`: Rebuild config from a toml file. See [config.toml](config.toml) for an example.

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
