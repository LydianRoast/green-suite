// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

use anyhow::{Context, Result, anyhow, ensure};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use std::fs::File;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

pub const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_RENDER_DIMENSION: u32 = 4096;

/// A local, CPU-rendered PDF. Open and render inside the same background worker.
pub struct PdfDocument {
    pdf: Pdf,
}

impl PdfDocument {
    pub fn open(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("Cannot open PDF {}", path.display()))?;
        ensure!(
            file.metadata()?.len() <= MAX_PDF_BYTES,
            "PDF exceeds the 100 MiB limit"
        );
        let mut data = Vec::new();
        file.take(MAX_PDF_BYTES + 1).read_to_end(&mut data)?;
        ensure!(
            data.len() as u64 <= MAX_PDF_BYTES,
            "PDF exceeds the 100 MiB limit"
        );
        ensure!(
            data.windows(5).take(1024).any(|s| s == b"%PDF-"),
            "File does not contain a PDF header"
        );
        guarded(|| {
            let pdf = Pdf::new(data).map_err(|e| anyhow!("Cannot parse PDF: {e:?}"))?;
            ensure!(
                !pdf.pages().is_empty() && pdf.pages().len() <= 10_000,
                "PDF must contain between 1 and 10,000 pages"
            );
            Ok(Self { pdf })
        })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Returns the rotated crop dimensions in PDF points (72 per inch).
    pub fn page_size(&self, index: usize) -> Result<[f32; 2]> {
        guarded(|| {
            let page = self
                .pdf
                .pages()
                .get(index)
                .context("PDF page is out of range")?;
            let (width, height) = page.render_dimensions();
            ensure!(
                width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0,
                "PDF page has invalid dimensions"
            );
            ensure!(
                width <= 1_000_000.0 && height <= 1_000_000.0,
                "PDF page is unreasonably large"
            );
            Ok([width, height])
        })
    }

    /// Opaque RGBA bytes on a white background; premultiplication has no effect.
    pub fn render(&self, page: usize, max_dimension: u32) -> Result<(usize, usize, Vec<u8>)> {
        ensure!(
            (1..=MAX_RENDER_DIMENSION).contains(&max_dimension),
            "Render size must be between 1 and 4096 pixels"
        );
        let [width, height] = self.page_size(page)?;
        let scale = max_dimension as f32 / width.max(height);
        ensure!(
            width * scale >= 1.0 && height * scale >= 1.0,
            "PDF page aspect ratio is too extreme to render"
        );
        guarded(|| {
            let pixmap = hayro::render(
                &self.pdf.pages()[page],
                &RenderCache::new(),
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &PixmapSettings {
                    x_scale: scale,
                    y_scale: scale,
                    bg_color: hayro::vello_cpu::color::palette::css::WHITE,
                },
            );
            let width = pixmap.width() as usize;
            let height = pixmap.height() as usize;
            ensure!(
                width > 0
                    && height > 0
                    && width <= MAX_RENDER_DIMENSION as usize
                    && height <= MAX_RENDER_DIMENSION as usize,
                "Renderer returned invalid dimensions"
            );
            Ok((width, height, pixmap.data_as_u8_slice().to_vec()))
        })
    }
}

fn guarded<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(operation))
        .map_err(|_| anyhow!("The PDF renderer could not process this document"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny self-authored PDF exercises the actual parser and rasterizer.
    fn fixture() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 4 0 R >>",
            "<< /Length 26 >>\nstream\n0 0 1 rg 20 20 80 50 re f\nendstream",
        ];
        let mut data = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = data.len();
        data.extend(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            data.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        data.extend(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        data
    }

    #[test]
    fn parses_and_renders_pdf_with_bounded_dimensions() {
        let path =
            std::env::temp_dir().join(format!("green-suite-pdf-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, fixture()).unwrap();
        let pdf = PdfDocument::open(&path).unwrap();
        assert_eq!(pdf.page_count(), 1);
        assert_eq!(pdf.page_size(0).unwrap(), [200.0, 100.0]);
        let (width, height, rgba) = pdf.render(0, 400).unwrap();
        assert_eq!((width, height, rgba.len()), (400, 200, 320_000));
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
        assert!(rgba.chunks_exact(4).any(|px| px[2] > 200 && px[0] < 20));
        assert!(pdf.render(1, 400).is_err());
        assert!(pdf.render(0, 4097).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_pdf_is_a_controlled_error() {
        let path =
            std::env::temp_dir().join(format!("green-suite-pdf-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"%PDF-1.4\nnot a document").unwrap();
        assert!(PdfDocument::open(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
