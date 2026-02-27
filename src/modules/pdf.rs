//! PDF text extraction using the pdf-extract crate.

use crate::core::error::{NyayaError, Result};

/// Extract text from a PDF file at the given path.
pub fn parse_pdf_file(path: &str) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| NyayaError::Config(format!("Failed to read PDF file: {}", e)))?;
    parse_pdf_bytes(&bytes)
}

/// Extract text from PDF bytes in memory.
pub fn parse_pdf_bytes(bytes: &[u8]) -> Result<String> {
    pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| NyayaError::Config(format!("PDF parse error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pdf_bytes_empty() {
        let result = parse_pdf_bytes(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pdf_bytes_invalid() {
        let result = parse_pdf_bytes(b"not a pdf file");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pdf_file_nonexistent() {
        let result = parse_pdf_file("/nonexistent/file.pdf");
        assert!(result.is_err());
    }
}
