use serde::{Deserialize, Serialize};

use crate::pea::objective::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionMethod {
    pub name: String,
    pub applicable_keywords: Vec<String>,
    pub subtasks: Vec<SubtaskTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskTemplate {
    pub description_template: String,
    pub task_type: TaskType,
    pub capability_required: Option<String>,
    pub depends_on_indices: Vec<usize>,
}

// ---------------------------------------------------------------------------
// HtnDecomposer
// ---------------------------------------------------------------------------

pub struct HtnDecomposer {
    pub methods: Vec<DecompositionMethod>,
}

impl Default for HtnDecomposer {
    fn default() -> Self {
        Self {
            methods: built_in_methods(),
        }
    }
}

impl HtnDecomposer {
    pub fn new(methods: Vec<DecompositionMethod>) -> Self {
        Self { methods }
    }

    /// Find all methods whose keywords appear (case-insensitive) in the task description.
    pub fn find_methods(&self, task_description: &str) -> Vec<&DecompositionMethod> {
        let lower = task_description.to_lowercase();
        self.methods
            .iter()
            .filter(|m| {
                m.applicable_keywords
                    .iter()
                    .any(|kw| lower.contains(&kw.to_lowercase()))
            })
            .collect()
    }

    /// Decompose a compound task using the first applicable method.
    ///
    /// Generates subtasks with hierarchical IDs (`parent.0`, `parent.1`, …).
    /// Tasks with no dependencies get `TaskStatus::Ready`; others get `Pending`.
    pub fn decompose(
        &self,
        task: &PeaTask,
        objective_id: &str,
        desire_id: &str,
    ) -> Option<Vec<PeaTask>> {
        let methods = self.find_methods(&task.description);
        let method = methods.first()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let subtasks: Vec<PeaTask> = method
            .subtasks
            .iter()
            .enumerate()
            .map(|(i, tpl)| {
                let id = format!("{}.{}", task.id, i);

                // Build dependency IDs from indices within the same method.
                let depends_on: Vec<String> = tpl
                    .depends_on_indices
                    .iter()
                    .map(|&idx| format!("{}.{}", task.id, idx))
                    .collect();

                let status = if depends_on.is_empty() {
                    TaskStatus::Ready
                } else {
                    TaskStatus::Pending
                };

                PeaTask {
                    id,
                    objective_id: objective_id.to_string(),
                    desire_id: desire_id.to_string(),
                    parent_task_id: Some(task.id.clone()),
                    description: tpl.description_template.clone(),
                    task_type: tpl.task_type.clone(),
                    status,
                    ordering: i as i32,
                    depends_on,
                    capability_required: tpl.capability_required.clone(),
                    result_json: None,
                    pramana_record_json: None,
                    retry_count: 0,
                    max_retries: 3,
                    created_at: now,
                    completed_at: None,
                }
            })
            .collect();

        Some(subtasks)
    }

    /// Promote any `Pending` task whose dependencies are all `Completed` or `Skipped`
    /// to `Ready`.
    pub fn promote_ready_tasks(tasks: &mut [PeaTask]) {
        // Collect resolved IDs into owned strings to avoid borrow issues.
        let resolved: std::collections::HashSet<String> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed || t.status == TaskStatus::Skipped)
            .map(|t| t.id.clone())
            .collect();

        for task in tasks.iter_mut() {
            if task.status == TaskStatus::Pending
                && !task.depends_on.is_empty()
                && task.depends_on.iter().all(|dep| resolved.contains(dep))
            {
                task.status = TaskStatus::Ready;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in methods
// ---------------------------------------------------------------------------

pub fn built_in_methods() -> Vec<DecompositionMethod> {
    vec![
        DecompositionMethod {
            name: "research_and_compile".into(),
            applicable_keywords: vec!["research".into(), "investigate".into(), "study".into()],
            subtasks: vec![
                SubtaskTemplate {
                    description_template: "Search web for sources".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![],
                },
                SubtaskTemplate {
                    description_template: "Search academic papers".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![],
                },
                SubtaskTemplate {
                    description_template: "Synthesize findings".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![0, 1],
                },
            ],
        },
        DecompositionMethod {
            name: "write_document".into(),
            applicable_keywords: vec![
                "write".into(),
                "draft".into(),
                "compose".into(),
                "author".into(),
            ],
            subtasks: vec![
                SubtaskTemplate {
                    description_template: "Create outline".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![],
                },
                SubtaskTemplate {
                    description_template: "Write content sections".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![0],
                },
                SubtaskTemplate {
                    description_template: "Review and refine".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![1],
                },
            ],
        },
        DecompositionMethod {
            name: "generate_media".into(),
            applicable_keywords: vec![
                "image".into(),
                "video".into(),
                "visual".into(),
                "photo".into(),
                "illustration".into(),
            ],
            subtasks: vec![
                SubtaskTemplate {
                    description_template: "Plan visual content".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![],
                },
                SubtaskTemplate {
                    description_template: "Generate using media engine".into(),
                    task_type: TaskType::Primitive,
                    capability_required: Some("media_engine".into()),
                    depends_on_indices: vec![0],
                },
                SubtaskTemplate {
                    description_template: "Review and select".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![1],
                },
            ],
        },
        DecompositionMethod {
            name: "publish_content".into(),
            applicable_keywords: vec![
                "publish".into(),
                "release".into(),
                "distribute".into(),
                "share".into(),
            ],
            subtasks: vec![
                SubtaskTemplate {
                    description_template: "Format for distribution".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![],
                },
                SubtaskTemplate {
                    description_template: "Upload/send to platform".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![0],
                },
                SubtaskTemplate {
                    description_template: "Announce on channels".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![1],
                },
            ],
        },
        DecompositionMethod {
            name: "social_media_campaign".into(),
            applicable_keywords: vec![
                "social media".into(),
                "promote".into(),
                "marketing".into(),
                "engagement".into(),
            ],
            subtasks: vec![
                SubtaskTemplate {
                    description_template: "Create content calendar".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![],
                },
                SubtaskTemplate {
                    description_template: "Generate post content and images".into(),
                    task_type: TaskType::Primitive,
                    capability_required: Some("media_engine".into()),
                    depends_on_indices: vec![0],
                },
                SubtaskTemplate {
                    description_template: "Schedule and post".into(),
                    task_type: TaskType::Primitive,
                    capability_required: None,
                    depends_on_indices: vec![1],
                },
            ],
        },
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, description: &str) -> PeaTask {
        PeaTask {
            id: id.into(),
            objective_id: "obj-1".into(),
            desire_id: "d-1".into(),
            parent_task_id: None,
            description: description.into(),
            task_type: TaskType::Compound,
            status: TaskStatus::Ready,
            ordering: 0,
            depends_on: vec![],
            capability_required: None,
            result_json: None,
            pramana_record_json: None,
            retry_count: 0,
            max_retries: 3,
            created_at: 100,
            completed_at: None,
        }
    }

    #[test]
    fn test_find_methods_by_keyword() {
        let decomposer = HtnDecomposer::default();
        let methods = decomposer.find_methods("research Indian recipes");
        assert!(!methods.is_empty());
        assert_eq!(methods[0].name, "research_and_compile");
    }

    #[test]
    fn test_find_methods_no_match() {
        let decomposer = HtnDecomposer::default();
        let methods = decomposer.find_methods("dance");
        assert!(methods.is_empty());
    }

    #[test]
    fn test_decompose_produces_correct_subtasks() {
        let decomposer = HtnDecomposer::default();
        let task = make_task("t1", "research quantum computing");
        let subtasks = decomposer.decompose(&task, "obj-1", "d-1").unwrap();

        assert_eq!(subtasks.len(), 3);

        // First two have no deps → Ready
        assert_eq!(subtasks[0].status, TaskStatus::Ready);
        assert_eq!(subtasks[1].status, TaskStatus::Ready);

        // Third depends on 0 and 1 → Pending
        assert_eq!(subtasks[2].status, TaskStatus::Pending);
        assert_eq!(subtasks[2].depends_on, vec!["t1.0", "t1.1"]);
    }

    #[test]
    fn test_decompose_subtask_ids_hierarchical() {
        let decomposer = HtnDecomposer::default();
        let task = make_task("t1", "research quantum computing");
        let subtasks = decomposer.decompose(&task, "obj-1", "d-1").unwrap();

        assert_eq!(subtasks[0].id, "t1.0");
        assert_eq!(subtasks[1].id, "t1.1");
        assert_eq!(subtasks[2].id, "t1.2");
    }

    #[test]
    fn test_decompose_no_method_returns_none() {
        let decomposer = HtnDecomposer::default();
        let task = make_task("t1", "unrelated gibberish");
        let result = decomposer.decompose(&task, "obj-1", "d-1");
        assert!(result.is_none());
    }

    #[test]
    fn test_promote_ready_tasks() {
        let decomposer = HtnDecomposer::default();
        let task = make_task("t1", "research quantum computing");
        let mut subtasks = decomposer.decompose(&task, "obj-1", "d-1").unwrap();

        // Initially task[2] is Pending
        assert_eq!(subtasks[2].status, TaskStatus::Pending);

        // Complete both dependencies
        subtasks[0].status = TaskStatus::Completed;
        subtasks[1].status = TaskStatus::Completed;

        HtnDecomposer::promote_ready_tasks(&mut subtasks);

        // Now task[2] should be Ready
        assert_eq!(subtasks[2].status, TaskStatus::Ready);
    }
}
