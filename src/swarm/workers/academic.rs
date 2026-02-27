use crate::swarm::worker::{SourceTarget, WorkerType};

/// Academic research worker — fetches papers from OpenAlex API.
pub struct AcademicWorker;

impl Default for AcademicWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl AcademicWorker {
    pub fn new() -> Self {
        Self
    }
}

impl WorkerType for AcademicWorker {
    fn name(&self) -> &str {
        "academic"
    }

    fn can_handle(&self, target: &SourceTarget) -> bool {
        matches!(target, SourceTarget::OpenAlexQuery(_))
    }

    fn max_pages(&self) -> usize {
        10
    }
}

impl std::fmt::Display for AcademicWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AcademicWorker(openalex)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_academic_worker_can_handle() {
        let w = AcademicWorker::new();
        assert!(w.can_handle(&SourceTarget::OpenAlexQuery("quantum".into())));
    }

    #[test]
    fn test_academic_worker_rejects_url() {
        let w = AcademicWorker::new();
        assert!(!w.can_handle(&SourceTarget::Url("https://example.com".into())));
    }
}
