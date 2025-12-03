use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::fnt::Fnt;
use crate::glyph::Glyph;

pub fn extract_fnt(fnt: &Fnt, output_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(output_dir)?;

    let lazy_glyphs = fnt.lazy_glyphs.clone();

    let metadata = fnt.metadata.clone();
    let metadata_path = output_dir.join("metadata.toml");
    metadata.write_metadata(&metadata_path)?;

    let total = lazy_glyphs.len();
    let counter = AtomicUsize::new(0);

    lazy_glyphs.par_iter().for_each(|(glyph_id, lazy_glyph)| {
        let glyph = Glyph::from_lazy_glyph(lazy_glyph, fnt.metadata.version);
        let info = &lazy_glyph.info;
        let filename = format!("{:04}_{:04x}_0.png", glyph_id, info.char_code);
        let glyph_path = output_dir.join(&filename);
        glyph.write_png(&glyph_path).unwrap();

        let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
        if done % 100 == 0 || done == total {
            print!(
                "\rExporting glyphs: {}/{} ({:.1}%)",
                done,
                total,
                done as f64 / total as f64 * 100.0
            );
            std::io::stdout().flush().ok();
        }
    });

    println!();

    Ok(())
}
