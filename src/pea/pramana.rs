use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Hetvabhasa — fallacies in inference (Nyaya epistemology).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Hetvabhasa {
    /// Unproved premise — the reason itself is not established.
    Asiddha(String),
    /// Contradictory reason — the reason proves the opposite.
    Viruddha(String),
    /// Irregular reason — the reason is inconclusive / deviating.
    Savyabhichara(String),
}

impl fmt::Display for Hetvabhasa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asiddha(s) => write!(f, "Asiddha (unproved premise): {s}"),
            Self::Viruddha(s) => write!(f, "Viruddha (contradictory reason): {s}"),
            Self::Savyabhichara(s) => write!(f, "Savyabhichara (irregular reason): {s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Result structs for each pramana
// ---------------------------------------------------------------------------

/// Pratyaksha — direct perception / observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PratyakshaResult {
    pub observation: String,
    pub matches_expectation: bool,
}

/// Anumana — inference / logical reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnumanaResult {
    pub reasoning: String,
    pub fallacies_detected: Vec<Hetvabhasa>,
    pub sound: bool,
}

/// Upamana — comparison / analogy with past experience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpamanaResult {
    pub analogous_episode: Option<String>,
    pub past_outcome: Option<String>,
    pub relevance_score: f64,
}

/// Shabda — testimony from a reliable authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShabdaResult {
    pub authority: String,
    pub testimony: String,
    pub pending: bool,
}

/// Aggregated record of all four pramana checks for a decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PramanaRecord {
    pub decision: String,
    pub pratyaksha: Option<PratyakshaResult>,
    pub anumana: Option<AnumanaResult>,
    pub upamana: Option<UpamanaResult>,
    pub shabda: Option<ShabdaResult>,
    pub confidence: f64,
    pub validated: bool,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// PramanaValidator
// ---------------------------------------------------------------------------

pub struct PramanaValidator {
    pub confidence_threshold: f64,
}

impl Default for PramanaValidator {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.7,
        }
    }
}

impl PramanaValidator {
    pub fn new(confidence_threshold: f64) -> Self {
        Self {
            confidence_threshold,
        }
    }

    /// Pratyaksha — direct observation check.
    /// Returns whether the observation contains the expected string.
    pub fn pratyaksha(&self, observation: &str, expectation: &str) -> PratyakshaResult {
        PratyakshaResult {
            observation: observation.to_string(),
            matches_expectation: observation.contains(expectation),
        }
    }

    /// Anumana — logical inference check.
    /// Detects Asiddha (premise not supported by evidence) and
    /// Viruddha (evidence contradicts conclusion).
    pub fn anumana(
        &self,
        premises: &[String],
        conclusion: &str,
        evidence: &[String],
    ) -> AnumanaResult {
        let mut fallacies = Vec::new();

        // Check Asiddha: each premise must be supported by at least one piece of evidence.
        for premise in premises {
            let supported = evidence.iter().any(|e| e.contains(premise.as_str()));
            if !supported {
                fallacies.push(Hetvabhasa::Asiddha(premise.clone()));
            }
        }

        // Check Viruddha: evidence explicitly negates the conclusion.
        let negation = format!("not {conclusion}");
        for ev in evidence {
            if ev.contains(&negation) {
                fallacies.push(Hetvabhasa::Viruddha(format!(
                    "evidence \"{ev}\" contradicts conclusion \"{conclusion}\""
                )));
            }
        }

        let sound = fallacies.is_empty();
        AnumanaResult {
            reasoning: format!(
                "Evaluated {} premises against {} evidence items → conclusion: {conclusion}",
                premises.len(),
                evidence.len()
            ),
            fallacies_detected: fallacies,
            sound,
        }
    }

    /// Upamana — analogy with past episodes.
    /// Finds the best matching past episode with relevance score > 0.6.
    pub fn upamana(
        &self,
        _current_task: &str,
        past_episodes: &[(String, String, f64)],
    ) -> UpamanaResult {
        let best = past_episodes
            .iter()
            .filter(|(_, _, score)| *score > 0.6)
            .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        match best {
            Some((episode, outcome, score)) => UpamanaResult {
                analogous_episode: Some(episode.clone()),
                past_outcome: Some(outcome.clone()),
                relevance_score: *score,
            },
            None => UpamanaResult {
                analogous_episode: None,
                past_outcome: None,
                relevance_score: 0.0,
            },
        }
    }

    /// Shabda — request testimony from a reliable authority.
    /// Creates a pending record awaiting response.
    pub fn shabda_request(&self, authority: &str, question: &str) -> ShabdaResult {
        ShabdaResult {
            authority: authority.to_string(),
            testimony: question.to_string(),
            pending: true,
        }
    }

    /// Aggregate all four pramana results into a single PramanaRecord.
    ///
    /// Weighted average:
    /// - pratyaksha: 1.0 if matches, 0.0 if not
    /// - anumana: 1.0 if sound, 0.2 if not
    /// - upamana: relevance_score
    /// - shabda: 0.9 if not pending
    ///
    /// If confidence < threshold and no shabda provided, auto-creates a shabda request.
    pub fn aggregate(
        &self,
        decision: &str,
        pratyaksha: Option<PratyakshaResult>,
        anumana: Option<AnumanaResult>,
        upamana: Option<UpamanaResult>,
        shabda: Option<ShabdaResult>,
    ) -> PramanaRecord {
        let mut scores: Vec<f64> = Vec::new();

        if let Some(ref p) = pratyaksha {
            scores.push(if p.matches_expectation { 1.0 } else { 0.0 });
        }
        if let Some(ref a) = anumana {
            scores.push(if a.sound { 1.0 } else { 0.2 });
        }
        if let Some(ref u) = upamana {
            scores.push(u.relevance_score);
        }
        if let Some(ref s) = shabda {
            if !s.pending {
                scores.push(0.9);
            }
        }

        let confidence = if scores.is_empty() {
            0.0
        } else {
            scores.iter().sum::<f64>() / scores.len() as f64
        };

        // If confidence is below threshold and no shabda was provided,
        // auto-create a shabda request for human review.
        let final_shabda = if confidence < self.confidence_threshold && shabda.is_none() {
            Some(self.shabda_request(
                "human_reviewer",
                &format!(
                    "Low confidence ({confidence:.2}) for decision: {decision}. Please review."
                ),
            ))
        } else {
            shabda
        };

        let validated = confidence >= self.confidence_threshold;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        PramanaRecord {
            decision: decision.to_string(),
            pratyaksha,
            anumana,
            upamana,
            shabda: final_shabda,
            confidence,
            validated,
            timestamp,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pratyaksha_matches() {
        let v = PramanaValidator::default();
        let result = v.pratyaksha("file exists at /tmp/test", "file exists");
        assert!(result.matches_expectation);
    }

    #[test]
    fn test_pratyaksha_mismatch() {
        let v = PramanaValidator::default();
        let result = v.pratyaksha("error occurred", "success");
        assert!(!result.matches_expectation);
    }

    #[test]
    fn test_anumana_sound() {
        let v = PramanaValidator::default();
        let premises = vec!["smoke is present".to_string()];
        let evidence = vec!["smoke is present in the room".to_string()];
        let result = v.anumana(&premises, "there is fire", &evidence);
        assert!(result.sound);
        assert!(result.fallacies_detected.is_empty());
    }

    #[test]
    fn test_anumana_asiddha() {
        let v = PramanaValidator::default();
        let premises = vec!["recipe is popular".to_string()];
        let evidence = vec!["ingredients are available".to_string()];
        let result = v.anumana(&premises, "dish will taste good", &evidence);
        assert!(!result.sound);
        assert!(result
            .fallacies_detected
            .iter()
            .any(|f| matches!(f, Hetvabhasa::Asiddha(_))));
    }

    #[test]
    fn test_anumana_viruddha() {
        let v = PramanaValidator::default();
        let premises = vec!["effort was made".to_string()];
        let evidence = vec![
            "effort was made consistently".to_string(),
            "not successful despite effort".to_string(),
        ];
        let result = v.anumana(&premises, "successful", &evidence);
        assert!(!result.sound);
        assert!(result
            .fallacies_detected
            .iter()
            .any(|f| matches!(f, Hetvabhasa::Viruddha(_))));
    }

    #[test]
    fn test_upamana_finds_analogy() {
        let v = PramanaValidator::default();
        let past = vec![
            ("research recipes".to_string(), "succeeded".to_string(), 0.8),
            ("clean house".to_string(), "partially done".to_string(), 0.3),
        ];
        let result = v.upamana("research ingredients", &past);
        assert_eq!(
            result.analogous_episode.as_deref(),
            Some("research recipes")
        );
        assert_eq!(result.past_outcome.as_deref(), Some("succeeded"));
        assert!((result.relevance_score - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_aggregate_low_confidence_triggers_shabda() {
        let v = PramanaValidator::default();
        let pratyaksha = Some(PratyakshaResult {
            observation: "error occurred".to_string(),
            matches_expectation: false,
        });
        let record = v.aggregate("deploy to production", pratyaksha, None, None, None);
        // pratyaksha gives 0.0 → confidence 0.0 < 0.7 threshold → shabda auto-created
        assert!(record.shabda.is_some());
        let shabda = record.shabda.unwrap();
        assert!(shabda.pending);
        assert_eq!(shabda.authority, "human_reviewer");
        assert!(!record.validated);
    }

    #[test]
    fn test_hetvabhasa_display() {
        let a = Hetvabhasa::Asiddha("premise unproved".to_string());
        let v = Hetvabhasa::Viruddha("contradicts conclusion".to_string());
        let s = Hetvabhasa::Savyabhichara("inconclusive reason".to_string());

        let a_str = a.to_string();
        let v_str = v.to_string();
        let s_str = s.to_string();

        assert!(a_str.contains("Asiddha") && a_str.contains("premise unproved"));
        assert!(v_str.contains("Viruddha") && v_str.contains("contradicts conclusion"));
        assert!(s_str.contains("Savyabhichara") && s_str.contains("inconclusive reason"));
    }
}
