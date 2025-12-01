use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{
    extract::{detect_mipmap_levels, extract_fnt, read_fnt},
    rebuild::rebuild_fnt,
    repack::{parse_metadata, process_glyphs, repack_fnt},
    types::FntVersion,
};

pub mod crc32;
pub mod extract;
pub mod lz77;
pub mod rebuild;
pub mod repack;
pub mod types;

#[derive(Parser, Debug)]
#[command(name = "fnt4-tool")]
#[command(author, version, about = "FNT4 font tools")]
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
        #[arg(short, long, default_value = "4")]
        mipmap_levels: usize,
    },

    /// Rebuild FNT4 font file from FNT4 font file and ttf font file (FNT4 V1 only)
    Rebuild {
        input_fnt: PathBuf,
        output_fnt: PathBuf,
        ttf_font: PathBuf,
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
            let mipmap_levels = detect_mipmap_levels(&fnt);

            println!("FNT4 version: {:?}", fnt.version);
            println!("Ascent: {}, Descent: {}", fnt.ascent, fnt.descent);
            println!("Total glyphs: {}", fnt.glyphs.len());
            println!("Mipmap levels: {}", mipmap_levels);

            println!("Extracting to: {:?}", output_dir);
            extract_fnt(&fnt, &output_dir)?;

            println!("Done!");
        }

        Commands::Repack {
            input_dir,
            output_fnt,
            mipmap_levels,
        } => {
            if mipmap_levels < 1 || mipmap_levels > 4 {
                return Err(anyhow::anyhow!(
                    "Invalid mipmap levels: {}. Must be 1-4.",
                    mipmap_levels
                ));
            }

            println!("Input directory: {:?}", input_dir);
            println!("Output FNT4 font: {:?}", output_fnt);
            println!("Mipmap levels: {}", mipmap_levels);

            let metadata_path = input_dir.join("metadata.txt");

            if !metadata_path.exists() {
                return Err(anyhow::anyhow!("metadata.txt not found in input directory"));
            }

            let metadata = parse_metadata(&metadata_path)?;
            let processed_glyphs = process_glyphs(
                input_dir.as_path(),
                &metadata,
                FntVersion::V1,
                mipmap_levels,
            )?;
            repack_fnt(&output_fnt.as_path(), &metadata, &processed_glyphs)?;

            println!("Done!");
        }
        Commands::Rebuild {
            input_fnt,
            output_fnt,
            ttf_font,
        } => {
            println!("Input FNT4 font: {:?}", input_fnt);
            println!("Output FNT4 font: {:?}", output_fnt);
            println!("TTF font: {:?}", ttf_font);

            let fnt_data = std::fs::read(&input_fnt)?;

            let fnt = read_fnt(&fnt_data).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse FNT4 font: {}", e),
                )
            })?;

            if fnt.version != FntVersion::V1 {
                return Err(anyhow::anyhow!("Rebuild only supported for FNT4 V1"));
            }

            println!("FNT4 version: {:?}", fnt.version);
            println!("Ascent: {}, Descent: {}", fnt.ascent, fnt.descent);
            println!("Total glyphs: {}", fnt.glyphs.len());

            rebuild_fnt(&fnt, &output_fnt, &ttf_font)?;

            println!("Done!");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FntVersion;
    use std::path::Path;

    fn test_fnt(fnt_path: &str, expected_version: FntVersion, expected_mipmap: usize) {
        let fnt_path = Path::new(fnt_path);
        if !fnt_path.exists() {
            println!("Skipping test: {} not found", fnt_path.display());
            return;
        }

        let fnt_name = fnt_path.file_stem().unwrap().to_str().unwrap();
        let temp_dir = Path::new("test_output").join(fnt_name);
        let output_fnt = temp_dir.with_extension("fnt");

        println!("\n=== Testing {} ===", fnt_name);
        let fnt_data = fs::read(fnt_path).expect("Failed to read FNT4 font");
        let fnt = read_fnt(&fnt_data).expect("Failed to parse FNT4 font");

        assert_eq!(fnt.version, expected_version);
        println!("Version: {:?}, Glyphs: {}", fnt.version, fnt.glyphs.len());
        let mipmap_levels = detect_mipmap_levels(&fnt);

        extract_fnt(&fnt, &temp_dir).expect("Failed to extract FNT4 glyphs");
        assert_eq!(mipmap_levels, expected_mipmap);
        println!("Mipmap levels: {}", mipmap_levels);

        if fnt.version == FntVersion::V1 {
            let metadata_path = temp_dir.join("metadata.txt");

            if !metadata_path.exists() {
                return;
            }

            let metadata = parse_metadata(&metadata_path).expect("Failed to parse metadata");
            let processed_glyphs =
                process_glyphs(temp_dir.as_path(), &metadata, FntVersion::V1, mipmap_levels)
                    .expect("failed to process glyphs");
            repack_fnt(output_fnt.as_path(), &metadata, &processed_glyphs)
                .expect("Failed to repack FNT4 font");

            let repacked_data = fs::read(&output_fnt).expect("Failed to read repacked FNT4 font");
            let repacked_fnt =
                read_fnt(&repacked_data).expect("Failed to parse repacked FNT4 font");

            assert_eq!(repacked_fnt.version, fnt.version);
            assert_eq!(repacked_fnt.glyphs.len(), fnt.glyphs.len());
            println!("Repack verified: {} glyphs", repacked_fnt.glyphs.len());
        }

        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::remove_file(&output_fnt);
        println!("Test passed!");
    }

    #[test]
    fn test() {
        test_fnt("sample/PCSG00997_FNT4_v0.fnt", FntVersion::V0, 1);
        test_fnt("sample/PCSG01258_FNT4_v1.fnt", FntVersion::V1, 4);
        test_fnt("sample/PCSG00901_FNT4_v1_mipmap3.fnt", FntVersion::V1, 3);
    }
}
