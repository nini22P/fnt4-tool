use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{
    extract::extract_fnt,
    fnt::Fnt,
    metadata::{FntMetadata, FntVersion},
    rebuild::{RebuildConfig, rebuild_fnt},
    repack::process_glyphs,
};

pub mod crc32;
pub mod extract;
pub mod fnt;
pub mod glyph;
pub mod lz77;
pub mod metadata;
pub mod rebuild;
pub mod repack;
pub mod utils;

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

    /// Rebuild FNT4 font file from FNT4 font file and TTF/OTF font file (FNT4 V1 only)
    Rebuild {
        input_fnt: PathBuf,
        output_fnt: PathBuf,
        source_font: PathBuf,
        /// Font size in pixels.
        /// If not specified, auto-calculated from original FNT (ascent + descent)
        #[arg(short = 's', long)]
        size: Option<f32>,
        /// Quality factor. Renders at higher resolution then downsamples with Lanczos filter.
        /// Higher = cleaner edges but slower. Recommended: 2-4. Default: 1 (no supersampling)
        #[arg(short = 'q', long)]
        quality: Option<u8>,
        /// Letter spacing pixels.
        /// Default: 0
        #[arg(long)]
        letter_spacing: Option<i8>,
        /// Texture padding pixels.
        /// If not specified, auto-calculated from original FNT (mipmap level)
        #[arg(long)]
        texture_padding: Option<u8>,
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

            let fnt = Fnt::read_fnt(&input_fnt)
                .map_err(|e| anyhow::anyhow!("Failed to parse FNT4 font: {}", e))?;

            println!("FNT4 version: {:?}", fnt.metadata.version);
            println!(
                "Ascent: {}, Descent: {}",
                fnt.metadata.ascent, fnt.metadata.descent
            );
            println!("Total glyphs: {}", fnt.metadata.glyphs.len());
            println!("Mipmap level: {}", fnt.metadata.mipmap_level);

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

            let metadata = FntMetadata::read_metadata(&metadata_path)?;
            println!("Ascent: {}, Descent: {}", metadata.ascent, metadata.descent);
            println!("Total glyphs: {}", metadata.glyphs.len());
            println!("Mipmap level: {}", metadata.mipmap_level);

            let processed_glyphs = process_glyphs(input_dir.as_path(), &metadata, FntVersion::V1)?;

            let fnt = Fnt::from_processed_glyphs(metadata, processed_glyphs);

            fnt.write_fnt(&output_fnt)?;

            println!("Done!");
        }
        Commands::Rebuild {
            input_fnt,
            output_fnt,
            source_font,
            size,
            quality,
            letter_spacing,
            texture_padding,
            config,
        } => {
            println!("Input FNT4 font: {:?}", input_fnt);
            println!("Output FNT4 font: {:?}", output_fnt);
            println!("Source font: {:?}", source_font);

            let fnt = Fnt::read_fnt(&input_fnt).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse FNT4 font: {}", e),
                )
            })?;

            println!("FNT4 version: {:?}", fnt.metadata.version);
            println!(
                "Ascent: {}, Descent: {}",
                fnt.metadata.ascent, fnt.metadata.descent
            );
            println!("Total glyphs: {}", fnt.metadata.glyphs.len());

            println!("Mipmap level: {}", fnt.metadata.mipmap_level);

            let mut config = if let Some(path) = config {
                println!("Config {:?}", path);
                RebuildConfig::load(&path)?
            } else {
                RebuildConfig::default()
            };

            if let Some(size) = size {
                config.size = Some(size);
            }

            if let Some(quality) = quality {
                config.quality = quality;
            }

            if let Some(letter_spacing) = letter_spacing {
                config.letter_spacing = letter_spacing;
            }

            if let Some(texture_padding) = texture_padding {
                config.texture_padding = Some(texture_padding);
            }

            rebuild_fnt(fnt, &output_fnt, &source_font, &config)?;

            println!("Done!");
        }
    }

    Ok(())
}
