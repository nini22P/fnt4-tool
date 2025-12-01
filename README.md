# fnt4-tool

FNT4 font extract/repack tool. Ported from [konosuba_py](https://github.com/lzhhzl/about-shin/tree/main/konosuba_py).

Only tested on *AstralAir no Shiroki Towa -White Eternity-*.

## Usage

```bash
# Extract
fnt4-tool extract input.fnt output_dir
# Output: Mipmap levels: N

# Repack (FNT4 V1 only, use -m value from extract output)
fnt4-tool repack input_dir output.fnt -m N
```

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
