//! CSV parsing and analysis.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub column_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    Numeric,
    Text,
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnStats {
    pub name: String,
    pub column_type: ColumnType,
    pub non_empty_count: usize,
    pub unique_count: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a CSV file at the given path.
pub fn parse_csv_file(path: &str) -> Result<CsvData> {
    let bytes = std::fs::read(path)
        .map_err(|e| NyayaError::Config(format!("Failed to read CSV file: {}", e)))?;
    parse_csv_bytes(&bytes)
}

/// Parse CSV from bytes in memory.
pub fn parse_csv_bytes(bytes: &[u8]) -> Result<CsvData> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(bytes);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| NyayaError::Config(format!("CSV header error: {}", e)))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    if headers.is_empty() {
        return Err(NyayaError::Config("CSV has no headers".into()));
    }

    let column_count = headers.len();
    let mut rows = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| NyayaError::Config(format!("CSV row error: {}", e)))?;
        let row: Vec<String> = record.iter().map(|f| f.to_string()).collect();
        rows.push(row);
    }

    let row_count = rows.len();

    Ok(CsvData {
        headers,
        rows,
        row_count,
        column_count,
    })
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Compute per-column statistics for parsed CSV data.
pub fn analyze(data: &CsvData) -> Vec<ColumnStats> {
    let mut stats = Vec::with_capacity(data.column_count);

    for col_idx in 0..data.column_count {
        let name = data.headers.get(col_idx).cloned().unwrap_or_default();

        let values: Vec<&str> = data
            .rows
            .iter()
            .filter_map(|row| row.get(col_idx).map(|s| s.as_str()))
            .collect();

        let non_empty: Vec<&str> = values
            .iter()
            .filter(|v| !v.trim().is_empty())
            .copied()
            .collect();

        let non_empty_count = non_empty.len();

        if non_empty_count == 0 {
            stats.push(ColumnStats {
                name,
                column_type: ColumnType::Empty,
                non_empty_count: 0,
                unique_count: 0,
                min: None,
                max: None,
                mean: None,
                median: None,
            });
            continue;
        }

        // Unique count
        let mut unique_set = std::collections::HashSet::new();
        for v in &non_empty {
            unique_set.insert(*v);
        }
        let unique_count = unique_set.len();

        // Try parsing all non-empty as f64
        let numeric_values: Vec<f64> = non_empty
            .iter()
            .filter_map(|v| v.trim().parse::<f64>().ok())
            .collect();

        let is_numeric = numeric_values.len() == non_empty_count && non_empty_count > 0;

        if is_numeric {
            let mut sorted = numeric_values.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            let min = sorted.first().copied();
            let max = sorted.last().copied();
            let sum: f64 = sorted.iter().sum();
            let mean = Some(sum / sorted.len() as f64);

            let median = if sorted.len() % 2 == 0 {
                let mid = sorted.len() / 2;
                Some((sorted[mid - 1] + sorted[mid]) / 2.0)
            } else {
                Some(sorted[sorted.len() / 2])
            };

            stats.push(ColumnStats {
                name,
                column_type: ColumnType::Numeric,
                non_empty_count,
                unique_count,
                min,
                max,
                mean,
                median,
            });
        } else {
            stats.push(ColumnStats {
                name,
                column_type: ColumnType::Text,
                non_empty_count,
                unique_count,
                min: None,
                max: None,
                mean: None,
                median: None,
            });
        }
    }

    stats
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_csv_with_headers() {
        let csv_bytes = b"name,age,city\nAlice,30,NYC\nBob,25,LA\n";
        let data = parse_csv_bytes(csv_bytes).unwrap();
        assert_eq!(data.headers, vec!["name", "age", "city"]);
        assert_eq!(data.row_count, 2);
        assert_eq!(data.column_count, 3);
        assert_eq!(data.rows[0], vec!["Alice", "30", "NYC"]);
        assert_eq!(data.rows[1], vec!["Bob", "25", "LA"]);
    }

    #[test]
    fn test_parse_csv_bytes_single_column() {
        let csv_bytes = b"value\n10\n20\n30\n";
        let data = parse_csv_bytes(csv_bytes).unwrap();
        assert_eq!(data.column_count, 1);
        assert_eq!(data.row_count, 3);
    }

    #[test]
    fn test_parse_csv_empty() {
        let csv_bytes = b"";
        let result = parse_csv_bytes(csv_bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_csv_file_nonexistent() {
        let result = parse_csv_file("/nonexistent/data.csv");
        assert!(result.is_err());
    }

    #[test]
    fn test_analyze_numeric_column() {
        let data = CsvData {
            headers: vec!["score".into()],
            rows: vec![
                vec!["10".into()],
                vec!["20".into()],
                vec!["30".into()],
                vec!["40".into()],
            ],
            row_count: 4,
            column_count: 1,
        };
        let stats = analyze(&data);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].column_type, ColumnType::Numeric);
        assert_eq!(stats[0].non_empty_count, 4);
        assert_eq!(stats[0].min, Some(10.0));
        assert_eq!(stats[0].max, Some(40.0));
        assert_eq!(stats[0].mean, Some(25.0));
        assert_eq!(stats[0].median, Some(25.0)); // (20+30)/2
    }

    #[test]
    fn test_analyze_text_column() {
        let data = CsvData {
            headers: vec!["name".into()],
            rows: vec![
                vec!["Alice".into()],
                vec!["Bob".into()],
                vec!["Alice".into()],
            ],
            row_count: 3,
            column_count: 1,
        };
        let stats = analyze(&data);
        assert_eq!(stats[0].column_type, ColumnType::Text);
        assert_eq!(stats[0].unique_count, 2);
        assert!(stats[0].min.is_none());
    }

    #[test]
    fn test_analyze_unique_count() {
        let data = CsvData {
            headers: vec!["city".into()],
            rows: vec![
                vec!["NYC".into()],
                vec!["LA".into()],
                vec!["NYC".into()],
                vec!["SF".into()],
                vec!["LA".into()],
            ],
            row_count: 5,
            column_count: 1,
        };
        let stats = analyze(&data);
        assert_eq!(stats[0].unique_count, 3); // NYC, LA, SF
        assert_eq!(stats[0].non_empty_count, 5);
    }
}
