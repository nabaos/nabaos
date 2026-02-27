//! Chart rendering via the `plotters` crate.
//!
//! Produces SVG byte vectors for line, bar, scatter, and candlestick charts.

use plotters::prelude::*;
use serde::Deserialize;

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The kind of chart to render.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Line,
    Bar,
    Scatter,
    Candlestick,
}

/// Configuration shared by every chart renderer.
#[derive(Debug, Clone, Deserialize)]
pub struct ChartConfig {
    pub chart_type: ChartType,
    pub title: String,
    #[serde(default = "default_x_label")]
    pub x_label: String,
    #[serde(default = "default_y_label")]
    pub y_label: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
}

fn default_x_label() -> String {
    "X".to_string()
}
fn default_y_label() -> String {
    "Y".to_string()
}
fn default_width() -> u32 {
    800
}
fn default_height() -> u32 {
    600
}

/// A named series of (x, y) data points.
#[derive(Debug, Clone)]
pub struct DataSeries {
    pub label: String,
    pub points: Vec<(f64, f64)>,
}

/// A single OHLC candlestick point.
#[derive(Debug, Clone)]
pub struct CandlestickPoint {
    pub x: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

// ---------------------------------------------------------------------------
// Color palette
// ---------------------------------------------------------------------------

const PALETTE: [RGBColor; 6] = [BLUE, RED, GREEN, MAGENTA, CYAN, BLACK];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the x and y ranges across all series, adding 10 % padding.
fn compute_range(series: &[DataSeries]) -> ((f64, f64), (f64, f64)) {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    for s in series {
        for &(x, y) in &s.points {
            if x < x_min {
                x_min = x;
            }
            if x > x_max {
                x_max = x;
            }
            if y < y_min {
                y_min = y;
            }
            if y > y_max {
                y_max = y;
            }
        }
    }

    let x_pad = (x_max - x_min).abs() * 0.1;
    let y_pad = (y_max - y_min).abs() * 0.1;

    // Guard against zero-range (single point)
    let x_pad = if x_pad == 0.0 { 1.0 } else { x_pad };
    let y_pad = if y_pad == 0.0 { 1.0 } else { y_pad };

    (
        (x_min - x_pad, x_max + x_pad),
        (y_min - y_pad, y_max + y_pad),
    )
}

/// Map a plotters `DrawingAreaErrorKind` into our error type.
fn chart_err<E: std::fmt::Debug>(e: E) -> NyayaError {
    NyayaError::Config(format!("Chart error: {e:?}"))
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

/// Render a line chart as SVG bytes.
pub fn render_line_chart(config: &ChartConfig, series: &[DataSeries]) -> Result<Vec<u8>> {
    if series.is_empty() || series.iter().all(|s| s.points.is_empty()) {
        return Err(NyayaError::Config("No data to render".to_string()));
    }

    let ((x_min, x_max), (y_min, y_max)) = compute_range(series);

    let mut buf = String::new();
    {
        let root =
            SVGBackend::with_string(&mut buf, (config.width, config.height)).into_drawing_area();
        root.fill(&WHITE).map_err(chart_err)?;

        let mut chart = ChartBuilder::on(&root)
            .caption(&config.title, ("sans-serif", 24))
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(x_min..x_max, y_min..y_max)
            .map_err(chart_err)?;

        chart
            .configure_mesh()
            .x_desc(&config.x_label)
            .y_desc(&config.y_label)
            .draw()
            .map_err(chart_err)?;

        for (i, s) in series.iter().enumerate() {
            let color = PALETTE[i % PALETTE.len()];
            let mut sorted = s.points.clone();
            sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            chart
                .draw_series(LineSeries::new(sorted, color.stroke_width(2)))
                .map_err(chart_err)?
                .label(&s.label)
                .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
        }

        chart
            .configure_series_labels()
            .border_style(BLACK)
            .draw()
            .map_err(chart_err)?;

        root.present().map_err(chart_err)?;
    }
    Ok(buf.into_bytes())
}

/// Render a bar chart as SVG bytes.
pub fn render_bar_chart(config: &ChartConfig, series: &[DataSeries]) -> Result<Vec<u8>> {
    if series.is_empty() || series.iter().all(|s| s.points.is_empty()) {
        return Err(NyayaError::Config("No data to render".to_string()));
    }

    let ((x_min, x_max), (y_min, y_max)) = compute_range(series);

    // Ensure y range includes 0 for meaningful bars
    let y_min = y_min.min(0.0);

    let mut buf = String::new();
    {
        let root =
            SVGBackend::with_string(&mut buf, (config.width, config.height)).into_drawing_area();
        root.fill(&WHITE).map_err(chart_err)?;

        let mut chart = ChartBuilder::on(&root)
            .caption(&config.title, ("sans-serif", 24))
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(x_min..x_max, y_min..y_max)
            .map_err(chart_err)?;

        chart
            .configure_mesh()
            .x_desc(&config.x_label)
            .y_desc(&config.y_label)
            .draw()
            .map_err(chart_err)?;

        let total_series = series.len();
        let bar_width = (x_max - x_min)
            / (series.iter().map(|s| s.points.len()).max().unwrap_or(1) as f64)
            * 0.6
            / total_series as f64;

        for (i, s) in series.iter().enumerate() {
            let color = PALETTE[i % PALETTE.len()];
            chart
                .draw_series(s.points.iter().map(|&(x, y)| {
                    let offset = (i as f64 - total_series as f64 / 2.0) * bar_width;
                    Rectangle::new(
                        [(x + offset, 0.0), (x + offset + bar_width, y)],
                        color.filled(),
                    )
                }))
                .map_err(chart_err)?
                .label(&s.label)
                .legend(move |(x, y)| {
                    Rectangle::new([(x, y - 5), (x + 15, y + 5)], color.filled())
                });
        }

        chart
            .configure_series_labels()
            .border_style(BLACK)
            .draw()
            .map_err(chart_err)?;

        root.present().map_err(chart_err)?;
    }
    Ok(buf.into_bytes())
}

/// Render a scatter chart as SVG bytes.
pub fn render_scatter_chart(config: &ChartConfig, series: &[DataSeries]) -> Result<Vec<u8>> {
    if series.is_empty() || series.iter().all(|s| s.points.is_empty()) {
        return Err(NyayaError::Config("No data to render".to_string()));
    }

    let ((x_min, x_max), (y_min, y_max)) = compute_range(series);

    let mut buf = String::new();
    {
        let root =
            SVGBackend::with_string(&mut buf, (config.width, config.height)).into_drawing_area();
        root.fill(&WHITE).map_err(chart_err)?;

        let mut chart = ChartBuilder::on(&root)
            .caption(&config.title, ("sans-serif", 24))
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(x_min..x_max, y_min..y_max)
            .map_err(chart_err)?;

        chart
            .configure_mesh()
            .x_desc(&config.x_label)
            .y_desc(&config.y_label)
            .draw()
            .map_err(chart_err)?;

        for (i, s) in series.iter().enumerate() {
            let color = PALETTE[i % PALETTE.len()];
            chart
                .draw_series(
                    s.points
                        .iter()
                        .map(|&(x, y)| Circle::new((x, y), 4, color.filled())),
                )
                .map_err(chart_err)?
                .label(&s.label)
                .legend(move |(x, y)| Circle::new((x + 10, y), 4, color.filled()));
        }

        chart
            .configure_series_labels()
            .border_style(BLACK)
            .draw()
            .map_err(chart_err)?;

        root.present().map_err(chart_err)?;
    }
    Ok(buf.into_bytes())
}

/// Render a candlestick (OHLC) chart as SVG bytes.
pub fn render_candlestick(config: &ChartConfig, points: &[CandlestickPoint]) -> Result<Vec<u8>> {
    if points.is_empty() {
        return Err(NyayaError::Config("No data to render".to_string()));
    }

    let x_min = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let x_max = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let y_min = points.iter().map(|p| p.low).fold(f64::INFINITY, f64::min);
    let y_max = points
        .iter()
        .map(|p| p.high)
        .fold(f64::NEG_INFINITY, f64::max);

    let x_pad = ((x_max - x_min).abs() * 0.1).max(1.0);
    let y_pad = ((y_max - y_min).abs() * 0.1).max(1.0);

    let mut buf = String::new();
    {
        let root =
            SVGBackend::with_string(&mut buf, (config.width, config.height)).into_drawing_area();
        root.fill(&WHITE).map_err(chart_err)?;

        let mut chart = ChartBuilder::on(&root)
            .caption(&config.title, ("sans-serif", 24))
            .margin(10)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(
                (x_min - x_pad)..(x_max + x_pad),
                (y_min - y_pad)..(y_max + y_pad),
            )
            .map_err(chart_err)?;

        chart
            .configure_mesh()
            .x_desc(&config.x_label)
            .y_desc(&config.y_label)
            .draw()
            .map_err(chart_err)?;

        chart
            .draw_series(points.iter().map(|p| {
                CandleStick::new(
                    p.x,
                    p.open,
                    p.high,
                    p.low,
                    p.close,
                    GREEN.filled(),
                    RED.filled(),
                    15,
                )
            }))
            .map_err(chart_err)?;

        root.present().map_err(chart_err)?;
    }
    Ok(buf.into_bytes())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_series() -> Vec<DataSeries> {
        vec![DataSeries {
            label: "Series A".to_string(),
            points: vec![(1.0, 2.0), (2.0, 4.0), (3.0, 3.0), (4.0, 5.0)],
        }]
    }

    #[test]
    fn test_line_chart_renders_svg() {
        let cfg = ChartConfig {
            chart_type: ChartType::Line,
            title: "My Line Chart".to_string(),
            x_label: "Time".to_string(),
            y_label: "Value".to_string(),
            width: 800,
            height: 600,
        };
        let svg = render_line_chart(&cfg, &sample_series()).expect("render should succeed");
        let text = String::from_utf8(svg).unwrap();
        assert!(text.contains("<svg"), "output must be SVG");
        assert!(
            text.contains("My Line Chart"),
            "output must contain the title"
        );
    }

    #[test]
    fn test_bar_chart_empty_data_returns_error() {
        let cfg = ChartConfig {
            chart_type: ChartType::Bar,
            title: "Empty".to_string(),
            x_label: "X".to_string(),
            y_label: "Y".to_string(),
            width: 800,
            height: 600,
        };
        let result = render_bar_chart(&cfg, &[]);
        assert!(result.is_err(), "empty series should produce an error");
    }

    #[test]
    fn test_scatter_chart_contains_title() {
        let cfg = ChartConfig {
            chart_type: ChartType::Scatter,
            title: "Scatter Example".to_string(),
            x_label: "X".to_string(),
            y_label: "Y".to_string(),
            width: 800,
            height: 600,
        };
        let svg = render_scatter_chart(&cfg, &sample_series()).expect("render should succeed");
        let text = String::from_utf8(svg).unwrap();
        assert!(
            text.contains("Scatter Example"),
            "output must contain the title"
        );
    }

    #[test]
    fn test_candlestick_renders() {
        let cfg = ChartConfig {
            chart_type: ChartType::Candlestick,
            title: "OHLC".to_string(),
            x_label: "Day".to_string(),
            y_label: "Price".to_string(),
            width: 800,
            height: 600,
        };
        let points = vec![
            CandlestickPoint {
                x: 1.0,
                open: 10.0,
                high: 15.0,
                low: 8.0,
                close: 12.0,
            },
            CandlestickPoint {
                x: 2.0,
                open: 12.0,
                high: 14.0,
                low: 9.0,
                close: 11.0,
            },
            CandlestickPoint {
                x: 3.0,
                open: 11.0,
                high: 16.0,
                low: 10.0,
                close: 15.0,
            },
        ];
        let svg = render_candlestick(&cfg, &points).expect("render should succeed");
        let text = String::from_utf8(svg).unwrap();
        assert!(text.contains("<svg"), "output must be SVG");
    }

    #[test]
    fn test_chart_config_defaults() {
        let json = r#"{"chart_type": "line", "title": "Defaults Test"}"#;
        let cfg: ChartConfig = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(cfg.title, "Defaults Test");
        assert_eq!(cfg.chart_type, ChartType::Line);
        assert_eq!(cfg.width, 800);
        assert_eq!(cfg.height, 600);
        assert_eq!(cfg.x_label, "X");
        assert_eq!(cfg.y_label, "Y");
    }
}
