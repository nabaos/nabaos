// PEA Charts — Rust-native chart generation using plotters.
//
// Provides a fallback for environments without Python/matplotlib.
// Generates publication-quality PRISMA flow diagrams and data charts
// as PNG images using the plotters crate.

#[cfg(feature = "charts")]
use plotters::prelude::*;

use std::path::{Path, PathBuf};

use crate::pea::research::{ResearchCorpus, SourceTier};

/// Check if matplotlib is available on this system.
pub fn has_matplotlib() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import matplotlib"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Generate a PRISMA 2020 flow diagram using plotters (Rust-native).
/// Returns the path to the generated PNG, or None on failure.
#[cfg(feature = "charts")]
pub fn generate_prisma_plotters(
    corpus: &ResearchCorpus,
    charts_dir: &Path,
) -> Option<(String, PathBuf, Option<String>)> {
    let path = charts_dir.join("prisma_flow.png");
    let _ = std::fs::create_dir_all(charts_dir);

    let total = corpus.total_candidates;
    let after_dedup = total.saturating_sub(corpus.duplicates_removed);
    let fetched = corpus.sources.len();
    let failed = corpus.failed_urls.len();
    let sought = fetched + failed;
    let excluded = after_dedup.saturating_sub(sought);

    let primary = corpus.sources.iter().filter(|s| s.tier == SourceTier::Primary).count();
    let analytical = corpus.sources.iter().filter(|s| s.tier == SourceTier::Analytical).count();
    let reporting = corpus.sources.iter().filter(|s| s.tier == SourceTier::Reporting).count();
    let aggregator = corpus.sources.iter().filter(|s| s.tier == SourceTier::Aggregator).count();

    {
        let root = BitMapBackend::new(&path, (900, 1050)).into_drawing_area();
        if root.fill(&WHITE).is_err() {
            return None;
        }

        // Helper: draw a filled rectangle with centered text
        let draw_box = |root: &DrawingArea<BitMapBackend, _>,
                        x: i32, y: i32, w: i32, h: i32,
                        text: &str, fill: &RGBColor| -> Result<(), Box<dyn std::error::Error>> {
            root.draw(&Rectangle::new(
                [(x, y), (x + w, y + h)],
                ShapeStyle::from(fill).filled(),
            ))?;
            root.draw(&Rectangle::new(
                [(x, y), (x + w, y + h)],
                ShapeStyle::from(&BLACK).stroke_width(1),
            ))?;
            let lines: Vec<&str> = text.lines().collect();
            let line_height = 16;
            let total_height = lines.len() as i32 * line_height;
            let start_y = y + (h - total_height) / 2;
            for (i, line) in lines.iter().enumerate() {
                let ty = start_y + i as i32 * line_height;
                root.draw(&Text::new(
                    line.to_string(),
                    (x + w / 2, ty),
                    ("serif", 12).into_font().color(&BLACK),
                ))?;
            }
            Ok(())
        };

        let _result = (|| -> Result<(), Box<dyn std::error::Error>> {
            // Title
            root.draw(&Text::new(
                "PRISMA 2020 Flow Diagram",
                (450, 20),
                ("serif", 18).into_font().color(&BLACK),
            ))?;

            let box_w = 280;
            let box_h = 70;
            let cx = 200;
            let rx = 580;
            let green = RGBColor(213, 245, 227);
            let light_blue = RGBColor(240, 244, 248);
            let light_red = RGBColor(250, 219, 216);

            // Phase labels
            root.draw(&Text::new("Identification", (20, 100), ("serif", 14).into_font().color(&BLACK)))?;
            root.draw(&Text::new("Screening", (20, 330), ("serif", 14).into_font().color(&BLACK)))?;
            root.draw(&Text::new("Included", (20, 700), ("serif", 14).into_font().color(&BLACK)))?;

            draw_box(&root, cx, 80, box_w, box_h,
                     &format!("Records identified\nthrough searching\n(n = {})", total),
                     &light_blue)?;

            root.draw(&PathElement::new(
                vec![(cx + box_w/2, 80 + box_h), (cx + box_w/2, 200)],
                ShapeStyle::from(&BLACK).stroke_width(2),
            ))?;

            draw_box(&root, cx, 200, box_w, box_h,
                     &format!("After deduplication\n(n = {})", after_dedup),
                     &light_blue)?;

            root.draw(&PathElement::new(
                vec![(cx + box_w/2, 200 + box_h), (cx + box_w/2, 320)],
                ShapeStyle::from(&BLACK).stroke_width(2),
            ))?;

            draw_box(&root, cx, 320, box_w, box_h,
                     &format!("Records screened\n(n = {})", after_dedup),
                     &light_blue)?;

            draw_box(&root, rx, 320, 250, box_h,
                     &format!("Excluded\n(below threshold)\n(n = {})", excluded),
                     &light_red)?;

            root.draw(&PathElement::new(
                vec![(cx + box_w, 320 + box_h/2), (rx, 320 + box_h/2)],
                ShapeStyle::from(&BLACK).stroke_width(1),
            ))?;

            root.draw(&PathElement::new(
                vec![(cx + box_w/2, 320 + box_h), (cx + box_w/2, 440)],
                ShapeStyle::from(&BLACK).stroke_width(2),
            ))?;

            draw_box(&root, cx, 440, box_w, box_h,
                     &format!("Sources sought for\nretrieval (n = {})", sought),
                     &light_blue)?;

            draw_box(&root, rx, 440, 250, box_h,
                     &format!("Not retrieved\n(HTTP error/timeout)\n(n = {})", failed),
                     &light_red)?;

            root.draw(&PathElement::new(
                vec![(cx + box_w, 440 + box_h/2), (rx, 440 + box_h/2)],
                ShapeStyle::from(&BLACK).stroke_width(1),
            ))?;

            root.draw(&PathElement::new(
                vec![(cx + box_w/2, 440 + box_h), (cx + box_w/2, 560)],
                ShapeStyle::from(&BLACK).stroke_width(2),
            ))?;

            draw_box(&root, cx, 560, box_w, box_h,
                     &format!("Sources assessed\nfor eligibility\n(n = {})", fetched),
                     &light_blue)?;

            root.draw(&PathElement::new(
                vec![(cx + box_w/2, 560 + box_h), (cx + box_w/2, 700)],
                ShapeStyle::from(&BLACK).stroke_width(2),
            ))?;

            draw_box(&root, cx, 700, box_w, 120,
                     &format!("Sources included\n(n = {})\n\nPrimary: {}\nAnalytical: {}\nReporting: {}\nAggregator: {}",
                         fetched, primary, analytical, reporting, aggregator),
                     &green)?;

            root.present()?;
            Ok(())
        })();
    } // root dropped here, releasing borrow on path

    if path.exists() {
        eprintln!("[charts] PRISMA flow diagram generated (plotters)");
        Some((
            "PRISMA 2020 Systematic Review Flow Diagram".to_string(),
            path,
            Some("Auto-generated from research pipeline data".to_string()),
        ))
    } else {
        None
    }
}

/// Generate a source distribution bar chart using plotters.
#[cfg(feature = "charts")]
pub fn generate_source_dist_plotters(
    corpus: &ResearchCorpus,
    charts_dir: &Path,
) -> Option<(String, PathBuf, Option<String>)> {
    if corpus.sources.len() < 3 {
        return None;
    }

    let path = charts_dir.join("source_distribution.png");
    let _ = std::fs::create_dir_all(charts_dir);

    let primary = corpus.sources.iter().filter(|s| s.tier == SourceTier::Primary).count() as u32;
    let analytical = corpus.sources.iter().filter(|s| s.tier == SourceTier::Analytical).count() as u32;
    let reporting = corpus.sources.iter().filter(|s| s.tier == SourceTier::Reporting).count() as u32;
    let aggregator = corpus.sources.iter().filter(|s| s.tier == SourceTier::Aggregator).count() as u32;

    let data = [
        ("Primary", primary, RGBColor(46, 204, 113)),
        ("Analytical", analytical, RGBColor(52, 152, 219)),
        ("Reporting", reporting, RGBColor(230, 126, 34)),
        ("Aggregator", aggregator, RGBColor(149, 165, 166)),
    ];

    let max_val = data.iter().map(|(_, v, _)| *v).max().unwrap_or(1);

    {
        let root = BitMapBackend::new(&path, (800, 500)).into_drawing_area();
        if root.fill(&WHITE).is_err() {
            return None;
        }

        let _result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut chart = ChartBuilder::on(&root)
                .caption("Source Distribution by Type", ("serif", 18))
                .margin(20)
                .x_label_area_size(40)
                .y_label_area_size(50)
                .build_cartesian_2d(
                    (0..3).into_segmented(),
                    0u32..(max_val + max_val / 5 + 1),
                )?;

            chart
                .configure_mesh()
                .disable_x_mesh()
                .y_desc("Number of Sources")
                .x_labels(4)
                .x_label_formatter(&|x| {
                    match x {
                        SegmentValue::CenterOf(i) => data.get(*i as usize)
                            .map(|(l, _, _)| l.to_string())
                            .unwrap_or_default(),
                        _ => String::new(),
                    }
                })
                .draw()?;

            chart.draw_series(
                data.iter().enumerate().map(|(i, (_, val, color))| {
                    Rectangle::new(
                        [
                            (SegmentValue::CenterOf(i as i32), 0),
                            (SegmentValue::CenterOf(i as i32), *val),
                        ],
                        color.filled(),
                    )
                }),
            )?;

            root.present()?;
            Ok(())
        })();
    } // root dropped here, releasing borrow on path

    if path.exists() {
        Some((
            "Source Distribution by Type".to_string(),
            path,
            Some("Auto-generated from research pipeline data".to_string()),
        ))
    } else {
        None
    }
}

#[cfg(not(feature = "charts"))]
pub fn generate_prisma_plotters(
    _corpus: &ResearchCorpus,
    _charts_dir: &Path,
) -> Option<(String, PathBuf, Option<String>)> {
    None
}

#[cfg(not(feature = "charts"))]
pub fn generate_source_dist_plotters(
    _corpus: &ResearchCorpus,
    _charts_dir: &Path,
) -> Option<(String, PathBuf, Option<String>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_matplotlib_runs() {
        // Just verify it doesn't panic
        let _ = has_matplotlib();
    }
}
