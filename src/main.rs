use std::{fs, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{
    extract::{extract_fnt, read_fnt},
    rebuild::rebuild_fnt,
    repack::process_glyphs,
    types::{Fnt, FntMetadata, FntVersion, RebuildConfig},
};

pub mod crc32;
pub mod extract;
pub mod fnt;
pub mod lz77;
pub mod metadata;
pub mod rebuild;
pub mod repack;
pub mod texture;
pub mod types;

#[derive(Parser, Debug)]
#[command(name = "fnt4-tool")]
#[command(author, version, about = "FNT4 font extract/repack/rebuild tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Extract FNT4 font file to PNG glyphs and metadata
    Extract {
        input_fnt: PathBuf,
        output_dir: PathBuf,
    },

    /// Repack PNG glyphs and metadata into FNT4 font file (FNT4 V1 only)
    Repack {
        input_dir: PathBuf,
        output_fnt: PathBuf,
    },

    /// Rebuild FNT4 font file from FNT4 font file and ttf/otf font file (FNT4 V1 only)
    Rebuild {
        input_fnt: PathBuf,
        output_fnt: PathBuf,
        source_font: PathBuf,
        /// Font size in pixels. If not specified, auto-calculated from original FNT (ascent + descent)
        #[arg(short = 's', long)]
        size: Option<f32>,
        /// Quality factor (1-8). Renders at higher resolution then downsamples with Lanczos filter.
        /// Higher = cleaner edges but slower. Recommended: 2-4. Default: 1 (no supersampling)
        #[arg(short = 'q', long, default_value = "1")]
        quality: u8,
        /// Padding pixels around each glyph. Prevents texture sampling artifacts at glyph edges.
        /// Default: 4
        #[arg(short = 'p', long, default_value = "4")]
        padding: u8,
        /// Rebuild config from a toml file.
        #[arg(short = 'c', long)]
        config: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract {
            input_fnt,
            output_dir,
        } => {
            println!("Reading FNT4 font: {:?}", input_fnt);
            let fnt_data = fs::read(&input_fnt)?;

            let fnt = read_fnt(&fnt_data)
                .map_err(|e| anyhow::anyhow!("Failed to parse FNT4 font: {}", e))?;
            let metadata = fnt.extract_metadata();

            println!("FNT4 version: {:?}", fnt.version);
            println!("Ascent: {}, Descent: {}", fnt.ascent, fnt.descent);
            println!("Total glyphs: {}", fnt.glyphs.len());
            println!("Mipmap levels: {}", metadata.mipmap_levels);

            println!("Extracting to: {:?}", output_dir);
            extract_fnt(&fnt, &output_dir)?;

            println!("Done!");
        }

        Commands::Repack {
            input_dir,
            output_fnt,
        } => {
            println!("Input directory: {:?}", input_dir);
            println!("Output FNT4 font: {:?}", output_fnt);

            let metadata_path = input_dir.join("metadata.toml");

            if !metadata_path.exists() {
                return Err(anyhow::anyhow!("metadata.txt not found in input directory"));
            }

            let metadata = FntMetadata::parse_metadata(&metadata_path)?;
            println!("Ascent: {}, Descent: {}", metadata.ascent, metadata.descent);
            println!("Total glyphs: {}", metadata.glyphs.len());
            println!("Mipmap levels: {}", metadata.mipmap_levels);

            let processed_glyphs = process_glyphs(input_dir.as_path(), &metadata, FntVersion::V1)?;

            let fnt = Fnt::from_processed_data(metadata, processed_glyphs, FntVersion::V1);

            fnt.save_fnt(&output_fnt)?;

            println!("Done!");
        }
        Commands::Rebuild {
            input_fnt,
            output_fnt,
            source_font,
            size,
            quality,
            padding,
            config,
        } => {
            println!("Input FNT4 font: {:?}", input_fnt);
            println!("Output FNT4 font: {:?}", output_fnt);
            println!("Source font: {:?}", source_font);

            let fnt_data = std::fs::read(&input_fnt)?;

            let fnt = read_fnt(&fnt_data).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse FNT4 font: {}", e),
                )
            })?;

            println!("FNT4 version: {:?}", fnt.version);
            println!("Ascent: {}, Descent: {}", fnt.ascent, fnt.descent);
            println!("Total glyphs: {}", fnt.glyphs.len());

            let config = if let Some(path) = config {
                println!("Config {:?}", path);
                Some(RebuildConfig::load(&path)?)
            } else {
                None
            };

            if let Some(config) = &config {
                println!("Hijack map: {} entries", config.hijack_map.len());
            }

            rebuild_fnt(
                fnt,
                &output_fnt,
                &source_font,
                size,
                quality,
                padding,
                config,
            )?;

            println!("Done!");
        }
    }

    Ok(())
}
