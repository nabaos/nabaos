// PEA Knowledge Graph — lightweight entity-relationship graph built from research.
//
// Extracts structured entities (people, organizations, events, locations, dates,
// statistics) from fetched sources and stores them as typed nodes with edges.
// Enables:
//   - Research reuse across related objectives
//   - Structural deduplication in the Nyaya trimmer (entity overlap scores)
//   - Cross-referencing between sections
//   - Fact verification against structured claims

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pea::research::{FetchedSource, ResearchCorpus};
use crate::runtime::host_functions::AbilityRegistry;
use crate::runtime::manifest::AgentManifest;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityType {
    Person,
    Organization,
    Event,
    Location,
    Date,
    Statistic,
}

impl EntityType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Organization => "org",
            Self::Event => "event",
            Self::Location => "location",
            Self::Date => "date",
            Self::Statistic => "stat",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Canonical name (lowercased, trimmed)
    pub name: String,
    pub entity_type: EntityType,
    /// Source URLs where this entity was found
    pub source_urls: Vec<String>,
    /// How many sources mention this entity
    pub mention_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// Source URL where this relationship was established
    pub source_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    /// Map from entity name → index in entities vec
    #[serde(skip)]
    entity_index: HashMap<String, usize>,
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            relationships: Vec::new(),
            entity_index: HashMap::new(),
        }
    }
}

impl KnowledgeGraph {
    /// Build KG from research corpus using LLM entity extraction.
    pub fn from_corpus(
        corpus: &ResearchCorpus,
        registry: &AbilityRegistry,
        manifest: &AgentManifest,
    ) -> Self {
        let mut kg = Self::default();

        if corpus.sources.is_empty() {
            return kg;
        }

        // Process sources in batches of 5 to avoid token limits
        let batch_size = 5;
        for batch in corpus.sources.chunks(batch_size) {
            let batch_text = batch
                .iter()
                .map(|s| {
                    let preview = if s.content.len() > 2000 {
                        format!(
                            "{}...",
                            crate::pea::research::safe_slice(&s.content, 2000)
                        )
                    } else {
                        s.content.clone()
                    };
                    format!("SOURCE [{}]:\n{}\n", s.url, preview)
                })
                .collect::<Vec<_>>()
                .join("\n---\n");

            let prompt = format!(
                "Extract named entities and relationships from these sources.\n\n\
                 {}\n\n\
                 For each source, extract:\n\
                 - People (names of individuals mentioned)\n\
                 - Organizations (companies, governments, institutions, agencies)\n\
                 - Events (named events, incidents, operations)\n\
                 - Locations (countries, cities, regions)\n\
                 - Dates (specific dates or date ranges mentioned)\n\
                 - Statistics (specific numbers, percentages, dollar amounts with context)\n\n\
                 Also extract relationships between entities:\n\
                 - subject → predicate → object (e.g. \"NATO\" → \"deployed forces to\" → \"Baltic States\")\n\n\
                 Respond with JSON:\n\
                 {{\n\
                   \"entities\": [\n\
                     {{\"name\": \"...\", \"type\": \"person|org|event|location|date|stat\", \"source\": \"url\"}}\n\
                   ],\n\
                   \"relationships\": [\n\
                     {{\"subject\": \"...\", \"predicate\": \"...\", \"object\": \"...\", \"source\": \"url\"}}\n\
                   ]\n\
                 }}\n\n\
                 Rules:\n\
                 - Normalize names: use full official names, not abbreviations\n\
                 - For statistics, include the number AND what it measures (e.g. \"$4.2 billion defense budget\")\n\
                 - Max 30 entities and 20 relationships per batch\n\
                 - Only extract entities that are factual, not speculative",
                batch_text,
            );

            let input = serde_json::json!({
                "system": "You are a named entity recognition specialist. Extract entities and \
                           relationships from text. Output ONLY valid JSON.",
                "prompt": prompt,
                "max_tokens": 4096,
                "thinking": false,
            });

            match registry.execute_ability(manifest, "llm.chat", &input.to_string()) {
                Ok(result) => {
                    let raw = String::from_utf8_lossy(&result.output).to_string();
                    let raw = crate::pea::composer::strip_thinking_tokens_pub(&raw);
                    kg.merge_extraction(&raw, batch);
                }
                Err(e) => {
                    eprintln!("[kg] entity extraction failed for batch: {}", e);
                }
            }
        }

        // Rebuild index after all merges
        kg.rebuild_index();

        eprintln!(
            "[kg] built knowledge graph: {} entities, {} relationships",
            kg.entities.len(),
            kg.relationships.len()
        );

        kg
    }

    /// Merge extracted entities/relationships from LLM JSON response.
    fn merge_extraction(&mut self, raw: &str, sources: &[FetchedSource]) {
        let json_str = crate::pea::composer::extract_json_pub(raw);
        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return,
        };

        // Default source URL for entities without explicit source
        let default_url = sources
            .first()
            .map(|s| s.url.as_str())
            .unwrap_or("");

        // Merge entities
        if let Some(entities) = parsed.get("entities").and_then(|e| e.as_array()) {
            for ent in entities {
                let name = match ent.get("name").and_then(|v| v.as_str()) {
                    Some(n) if !n.is_empty() => n.trim().to_string(),
                    _ => continue,
                };
                let entity_type = match ent
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("event")
                {
                    "person" => EntityType::Person,
                    "org" | "organization" => EntityType::Organization,
                    "location" => EntityType::Location,
                    "date" => EntityType::Date,
                    "stat" | "statistic" => EntityType::Statistic,
                    _ => EntityType::Event,
                };
                let source_url = ent
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or(default_url)
                    .to_string();

                let canonical = name.to_lowercase();
                if let Some(&idx) = self.entity_index.get(&canonical) {
                    // Merge: increment count, add source
                    self.entities[idx].mention_count += 1;
                    if !self.entities[idx].source_urls.contains(&source_url) {
                        self.entities[idx].source_urls.push(source_url);
                    }
                } else {
                    let idx = self.entities.len();
                    self.entity_index.insert(canonical, idx);
                    self.entities.push(Entity {
                        name,
                        entity_type,
                        source_urls: vec![source_url],
                        mention_count: 1,
                    });
                }
            }
        }

        // Merge relationships
        if let Some(rels) = parsed.get("relationships").and_then(|r| r.as_array()) {
            for rel in rels {
                let subject = match rel.get("subject").and_then(|v| v.as_str()) {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                let predicate = match rel.get("predicate").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => p.to_string(),
                    _ => continue,
                };
                let object = match rel.get("object").and_then(|v| v.as_str()) {
                    Some(o) if !o.is_empty() => o.to_string(),
                    _ => continue,
                };
                let source_url = rel
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or(default_url)
                    .to_string();

                // Dedup: skip if exact triple already exists
                let is_dup = self.relationships.iter().any(|r| {
                    r.subject.to_lowercase() == subject.to_lowercase()
                        && r.predicate.to_lowercase() == predicate.to_lowercase()
                        && r.object.to_lowercase() == object.to_lowercase()
                });
                if !is_dup {
                    self.relationships.push(Relationship {
                        subject,
                        predicate,
                        object,
                        source_url,
                    });
                }
            }
        }
    }

    /// Rebuild the entity name → index lookup.
    fn rebuild_index(&mut self) {
        self.entity_index.clear();
        for (i, ent) in self.entities.iter().enumerate() {
            self.entity_index
                .insert(ent.name.to_lowercase(), i);
        }
    }

    /// Compute entity overlap between two text sections.
    /// Returns (overlap_count, section_a_entities, section_b_entities).
    pub fn entity_overlap(&self, text_a: &str, text_b: &str) -> (usize, usize, usize) {
        let lower_a = text_a.to_lowercase();
        let lower_b = text_b.to_lowercase();

        let entities_a: HashSet<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| lower_a.contains(&e.name.to_lowercase()))
            .map(|(i, _)| i)
            .collect();

        let entities_b: HashSet<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| lower_b.contains(&e.name.to_lowercase()))
            .map(|(i, _)| i)
            .collect();

        let overlap = entities_a.intersection(&entities_b).count();
        (overlap, entities_a.len(), entities_b.len())
    }

    /// Compute entity overlap ratio between two texts.
    /// Returns 0.0-1.0 (Jaccard similarity of entity sets).
    pub fn overlap_ratio(&self, text_a: &str, text_b: &str) -> f64 {
        let lower_a = text_a.to_lowercase();
        let lower_b = text_b.to_lowercase();

        let entities_a: HashSet<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| lower_a.contains(&e.name.to_lowercase()))
            .map(|(i, _)| i)
            .collect();

        let entities_b: HashSet<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| lower_b.contains(&e.name.to_lowercase()))
            .map(|(i, _)| i)
            .collect();

        let union_size = entities_a.union(&entities_b).count();
        if union_size == 0 {
            return 0.0;
        }
        let overlap = entities_a.intersection(&entities_b).count();
        overlap as f64 / union_size as f64
    }

    /// Get entities mentioned in a text, sorted by mention count (most prominent first).
    pub fn entities_in_text(&self, text: &str) -> Vec<&Entity> {
        let lower = text.to_lowercase();
        let mut found: Vec<&Entity> = self
            .entities
            .iter()
            .filter(|e| lower.contains(&e.name.to_lowercase()))
            .collect();
        found.sort_by(|a, b| b.mention_count.cmp(&a.mention_count));
        found
    }

    /// Save KG to disk as JSON.
    pub fn save_to_disk(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load KG from disk.
    pub fn load_from_disk(path: &Path) -> Option<Self> {
        let json = std::fs::read_to_string(path).ok()?;
        let mut kg: Self = serde_json::from_str(&json).ok()?;
        kg.rebuild_index();
        Some(kg)
    }

    /// Format KG summary for LLM context.
    pub fn to_context_summary(&self) -> String {
        if self.entities.is_empty() {
            return String::new();
        }

        let mut summary = format!(
            "# Knowledge Graph ({} entities, {} relationships)\n\n",
            self.entities.len(),
            self.relationships.len()
        );

        // Group entities by type
        let types = [
            EntityType::Person,
            EntityType::Organization,
            EntityType::Event,
            EntityType::Location,
            EntityType::Date,
            EntityType::Statistic,
        ];
        for et in &types {
            let of_type: Vec<&Entity> = self
                .entities
                .iter()
                .filter(|e| &e.entity_type == et)
                .collect();
            if of_type.is_empty() {
                continue;
            }
            summary.push_str(&format!("## {} ({})\n", et.label(), of_type.len()));
            for e in of_type.iter().take(15) {
                summary.push_str(&format!(
                    "- {} (mentioned in {} sources)\n",
                    e.name, e.mention_count
                ));
            }
            if of_type.len() > 15 {
                summary.push_str(&format!("  ... and {} more\n", of_type.len() - 15));
            }
            summary.push('\n');
        }

        // Top relationships
        if !self.relationships.is_empty() {
            summary.push_str("## Key Relationships\n");
            for rel in self.relationships.iter().take(20) {
                summary.push_str(&format!(
                    "- {} → {} → {}\n",
                    rel.subject, rel.predicate, rel.object
                ));
            }
        }

        summary
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::default();
        kg.entities.push(Entity {
            name: "NATO".to_string(),
            entity_type: EntityType::Organization,
            source_urls: vec!["https://example.com".to_string()],
            mention_count: 3,
        });
        kg.entities.push(Entity {
            name: "Ukraine".to_string(),
            entity_type: EntityType::Location,
            source_urls: vec!["https://example.com".to_string()],
            mention_count: 5,
        });
        kg.entities.push(Entity {
            name: "Volodymyr Zelenskyy".to_string(),
            entity_type: EntityType::Person,
            source_urls: vec!["https://example.com".to_string()],
            mention_count: 4,
        });
        kg.entities.push(Entity {
            name: "European Union".to_string(),
            entity_type: EntityType::Organization,
            source_urls: vec!["https://example.com".to_string()],
            mention_count: 2,
        });
        kg.rebuild_index();
        kg
    }

    #[test]
    fn test_entity_overlap_full() {
        let kg = make_kg();
        let text_a = "NATO deployed forces to support Ukraine's defense.";
        let text_b = "Ukraine received support from NATO and the European Union.";
        let (overlap, a_count, b_count) = kg.entity_overlap(text_a, text_b);
        assert_eq!(overlap, 2); // NATO and Ukraine in both
        assert_eq!(a_count, 2); // NATO, Ukraine
        assert_eq!(b_count, 3); // NATO, Ukraine, European Union
    }

    #[test]
    fn test_overlap_ratio() {
        let kg = make_kg();
        let text_a = "NATO and Ukraine signed agreement";
        let text_b = "NATO and Ukraine discussed terms with the European Union";
        let ratio = kg.overlap_ratio(text_a, text_b);
        // a has {NATO, Ukraine}, b has {NATO, Ukraine, European Union}
        // overlap=2, union=3, ratio=0.667
        assert!(ratio > 0.6 && ratio < 0.7);
    }

    #[test]
    fn test_overlap_ratio_disjoint() {
        let kg = make_kg();
        let text_a = "NATO forces deployed";
        let text_b = "Zelenskyy met with European Union leaders";
        let ratio = kg.overlap_ratio(text_a, text_b);
        // a={NATO}, b={Zelenskyy, EU}, overlap=0, union=3
        assert!(ratio < 0.01);
    }

    #[test]
    fn test_entities_in_text() {
        let kg = make_kg();
        let text = "Ukraine's president Volodymyr Zelenskyy addressed NATO.";
        let found = kg.entities_in_text(text);
        assert_eq!(found.len(), 3);
        // Sorted by mention_count desc: Ukraine(5), Zelenskyy(4), NATO(3)
        assert_eq!(found[0].name, "Ukraine");
        assert_eq!(found[1].name, "Volodymyr Zelenskyy");
        assert_eq!(found[2].name, "NATO");
    }

    #[test]
    fn test_kg_serialization_roundtrip() {
        let kg = make_kg();
        let json = serde_json::to_string(&kg).unwrap();
        let mut loaded: KnowledgeGraph = serde_json::from_str(&json).unwrap();
        loaded.rebuild_index();
        assert_eq!(loaded.entities.len(), 4);
        assert_eq!(loaded.entities[0].name, "NATO");
    }

    #[test]
    fn test_context_summary() {
        let kg = make_kg();
        let summary = kg.to_context_summary();
        assert!(summary.contains("Knowledge Graph"));
        assert!(summary.contains("NATO"));
        assert!(summary.contains("person"));
        assert!(summary.contains("org"));
    }

    #[test]
    fn test_empty_kg() {
        let kg = KnowledgeGraph::default();
        assert_eq!(kg.entities.len(), 0);
        assert_eq!(kg.overlap_ratio("any text", "other text"), 0.0);
        assert!(kg.to_context_summary().is_empty());
    }
}
