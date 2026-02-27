use crate::swarm::worker::{SourceTarget, WorkerType};

/// PDF processing worker — downloads and extracts text from PDFs.
pub struct PdfWorker;

impl Default for PdfWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfWorker {
    pub fn new() -> Self {
        Self
    }
}

impl WorkerType for PdfWorker {
    fn name(&self) -> &str {
        "pdf"
    }

    fn can_handle(&self, target: &SourceTarget) -> bool {
        match target {
            SourceTarget::Url(url) => {
                url.to_lowercase().ends_with(".pdf")
                    || url.contains("/pdf/")
                    || url.contains("pdf?")
            }
            _ => false,
        }
    }

    fn max_pages(&self) -> usize {
        50 // pages of a PDF
    }
}

impl std::fmt::Display for PdfWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PdfWorker")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_worker_can_handle() {
        let w = PdfWorker::new();
        assert!(w.can_handle(&SourceTarget::Url("https://arxiv.org/paper.pdf".into())));
    }

    #[test]
    fn test_pdf_worker_rejects_non_pdf() {
        let w = PdfWorker::new();
        assert!(!w.can_handle(&SourceTarget::Url("https://example.com/page.html".into())));
    }
}
