use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{
    extract::{export_font, read_font},
    repack::repack_font,
};

pub mod crc32;
pub mod extract;
pub mod lz77;
pub mod repack;
pub mod types;

#[derive(Parser, Debug)]
#[command(name = "fnt4_tool")]
#[command(author, version, about = "FNT4 font tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Extract FNT4 font file to PNG glyphs and metadata
    Extract { input: PathBuf, output: PathBuf },

    /// Repack PNG glyphs and metadata into FNT4 font file (V1 only)
    Repack {
        input: PathBuf,
        output: PathBuf,
        #[arg(short, long, default_value = "4")]
        mipmap: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract { input, output } => {
            println!("Reading font file: {:?}", input);
            let font_data = fs::read(&input)?;

            let font = read_font(&font_data)
                .map_err(|e| anyhow::anyhow!("Failed to parse font: {}", e))?;

            println!("Font version: {:?}", font.version);
            println!("Ascent: {}, Descent: {}", font.ascent, font.descent);
            println!("Total glyphs: {}", font.glyphs.len());

            println!("Exporting to: {:?}", output);
            let mipmap_levels = export_font(&font, &output)?;

            println!("Mipmap levels: {}", mipmap_levels);
            println!("Done!");
        }

        Commands::Repack {
            input,
            output,
            mipmap,
        } => {
            if mipmap < 1 || mipmap > 4 {
                return Err(anyhow::anyhow!(
                    "Invalid mipmap levels: {}. Must be 1-4.",
                    mipmap
                ));
            }

            println!("Input directory: {:?}", input);
            println!("Output file: {:?}", output);
            println!("Mipmap levels: {}", mipmap);

            repack_font(&input, &output, mipmap)?;

            println!("Done!");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FontVersion;
    use std::path::Path;

    fn test_font(font_path: &str, expected_version: FontVersion, expected_mipmap: usize) {
        let font_path = Path::new(font_path);
        if !font_path.exists() {
            println!("Skipping test: {} not found", font_path.display());
            return;
        }

        let font_name = font_path.file_stem().unwrap().to_str().unwrap();
        let temp_dir = Path::new("test_output").join(font_name);
        let output_fnt = temp_dir.with_extension("fnt");

        println!("\n=== Testing {} ===", font_name);
        let font_data = fs::read(font_path).expect("Failed to read font file");
        let font = read_font(&font_data).expect("Failed to parse font");

        assert_eq!(font.version, expected_version);
        println!("Version: {:?}, Glyphs: {}", font.version, font.glyphs.len());

        let mipmap_levels = export_font(&font, &temp_dir).expect("Failed to export font");
        assert_eq!(mipmap_levels, expected_mipmap);
        println!("Mipmap levels: {}", mipmap_levels);

        if font.version == FontVersion::V1 {
            repack_font(&temp_dir, &output_fnt, mipmap_levels).expect("Failed to repack font");

            let repacked_data = fs::read(&output_fnt).expect("Failed to read repacked font");
            let repacked_font = read_font(&repacked_data).expect("Failed to parse repacked font");

            assert_eq!(repacked_font.version, font.version);
            assert_eq!(repacked_font.glyphs.len(), font.glyphs.len());
            println!("Repack verified: {} glyphs", repacked_font.glyphs.len());
        }

        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::remove_file(&output_fnt);
        println!("Test passed!");
    }

    #[test]
    fn test() {
        test_font("sample/PCSG00997_FNT4_v0.fnt", FontVersion::V0, 1);
        test_font("sample/PCSG01258_FNT4_v1.fnt", FontVersion::V1, 4);
        test_font("sample/PCSG00901_FNT4_v1_mipmap3.fnt", FontVersion::V1, 3);
    }
}
