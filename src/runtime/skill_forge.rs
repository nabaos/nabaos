//! Skill Forge — runtime skill creation with tiered privilege requirements.
//!
//! Three tiers of increasing privilege:
//! - **Chain**: Compose existing abilities into a workflow. No 2FA required.
//! - **Wasm**: Custom logic Wasm module. Level 1 2FA (TOTP) required.
//! - **Shell**: Shell script execution. Level 2 2FA (TOTP + password) required.

use crate::chain::workflow_store::WorkflowStore;
use crate::meta_agent::generator::WorkflowGenerator;
use crate::meta_agent::template_library::TemplateLibrary;
use std::path::Path;

/// Tier of skill creation.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SkillTier {
    /// Compose existing abilities. No 2FA required.
    Chain,
    /// Custom logic Wasm module. Level 1 2FA (TOTP) required.
    Wasm,
    /// Shell script. Level 2 2FA (TOTP + password) required. Exceptional only.
    Shell,
}

impl std::fmt::Display for SkillTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillTier::Chain => write!(f, "Workflow"),
            SkillTier::Wasm => write!(f, "Wasm"),
            SkillTier::Shell => write!(f, "Shell"),
        }
    }
}

/// Result of forging a skill.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ForgedSkill {
    /// Name of the forged skill.
    pub name: String,
    /// Tier at which the skill was forged.
    pub tier: SkillTier,
    /// Workflow ID if this is a Chain skill.
    pub workflow_id: Option<String>,
    /// Path to the Wasm module if this is a Wasm skill.
    pub wasm_path: Option<String>,
    /// Path to the shell script if this is a Shell skill.
    pub script_path: Option<String>,
}

/// Sanitize a human-readable name into a safe skill identifier.
///
/// Replaces non-alphanumeric characters (except `_` and `-`) with `_`,
/// lowercases everything, and trims leading/trailing underscores.
fn sanitize_skill_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Runtime skill creation engine.
///
/// Creates new skills at runtime with three tiers of increasing privilege
/// requirements. Chain skills compose existing abilities and require no
/// special authentication. Wasm and Shell tiers require progressively
/// stronger 2FA verification.
pub struct SkillForge;

impl SkillForge {
    /// Forge a Chain skill — compose existing abilities into a workflow.
    ///
    /// Uses the `WorkflowGenerator` to create a workflow definition from
    /// a natural language requirement, then stores it in the `WorkflowStore`.
    /// No 2FA is required for chain-tier skills.
    pub fn forge_chain(
        requirement: &str,
        name: &str,
        generator: &WorkflowGenerator,
        templates: &TemplateLibrary,
        store: &WorkflowStore,
    ) -> Result<ForgedSkill, String> {
        // Generate a workflow definition from the requirement.
        let mut def = generator.generate(requirement, templates)?;

        // Override the definition's id and name with the skill name.
        let skill_id = sanitize_skill_id(name);
        def.id = skill_id.clone();
        def.name = name.to_string();

        // Persist the workflow definition.
        store
            .store_def(&def)
            .map_err(|e| format!("Failed to store workflow definition: {}", e))?;

        Ok(ForgedSkill {
            name: name.to_string(),
            tier: SkillTier::Chain,
            workflow_id: Some(skill_id),
            wasm_path: None,
            script_path: None,
        })
    }

    /// Forge a Wasm skill — stub for future implementation.
    ///
    /// Wasm skills require Level 1 (TOTP) authentication and are not yet
    /// fully implemented. Returns an error directing users to the chain tier.
    pub fn forge_wasm(
        _requirement: &str,
        _name: &str,
        _skills_dir: &Path,
    ) -> Result<ForgedSkill, String> {
        Err(
            "Wasm skill forge requires Level 1 (TOTP) authentication and is not yet fully implemented. Use workflow tier."
                .to_string(),
        )
    }

    /// Forge a Shell skill — returns script content for user review before execution.
    ///
    /// Shell skills require Level 2 (TOTP + password) authentication. This method
    /// generates a safe placeholder script and returns it along with the `ForgedSkill`
    /// metadata. The caller is responsible for writing the script to disk after the
    /// user reviews and confirms the content.
    pub fn forge_shell(
        requirement: &str,
        name: &str,
        scripts_dir: &Path,
    ) -> Result<(ForgedSkill, String), String> {
        let skill_id = sanitize_skill_id(name);
        let script_content = format!(
            "#!/bin/bash\n# Skill: {}\n# Requirement: {}\necho 'Skill not yet implemented - review and edit this script'\n",
            name, requirement
        );

        let script_path = scripts_dir.join(format!("{}.sh", skill_id));
        let script_path_str = script_path
            .to_str()
            .ok_or_else(|| "Invalid script path encoding".to_string())?
            .to_string();

        let forged = ForgedSkill {
            name: name.to_string(),
            tier: SkillTier::Shell,
            workflow_id: None,
            wasm_path: None,
            script_path: Some(script_path_str),
        };

        Ok((forged, script_content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_agent::capability_index::{CapabilityIndex, WorkflowSummary};
    use crate::meta_agent::generator::WorkflowGenerator;
    use crate::meta_agent::template_library::TemplateLibrary;
    use crate::runtime::host_functions::AbilitySpec;
    use crate::runtime::plugin::AbilitySource;

    #[test]
    fn test_sanitize_skill_id() {
        // "My Cool Skill!" -> spaces become '_', '!' becomes '_', then trim trailing '_'
        assert_eq!(sanitize_skill_id("My Cool Skill!"), "my_cool_skill");
    }

    #[test]
    fn test_sanitize_skill_id_special_chars() {
        assert_eq!(sanitize_skill_id("test@#$%skill"), "test____skill");
    }

    #[test]
    fn test_forge_chain_uses_generator() {
        // Build a CapabilityIndex with demo abilities.
        let abilities = vec![
            AbilitySpec {
                name: "api.call".to_string(),
                description: "Call an API endpoint".to_string(),
                permission: "api.call".to_string(),
                source: AbilitySource::BuiltIn,
                input_schema: None,
            },
            AbilitySpec {
                name: "email.send".to_string(),
                description: "Send an email".to_string(),
                permission: "email.send".to_string(),
                source: AbilitySource::BuiltIn,
                input_schema: None,
            },
            AbilitySpec {
                name: "shopify.get_order".to_string(),
                description: "Fetch Shopify order details".to_string(),
                permission: "shopify.get_order".to_string(),
                source: AbilitySource::BuiltIn,
                input_schema: None,
            },
        ];

        let workflows = vec![WorkflowSummary {
            id: "demo_wf".into(),
            name: "Demo Workflow".into(),
            description: "A demo workflow".into(),
            node_count: 2,
        }];

        let index = CapabilityIndex::build(&abilities, workflows, &[]);
        let generator = WorkflowGenerator::new(&index);
        let templates = TemplateLibrary::new();

        // Use a temp file for the WorkflowStore.
        let tmp = std::env::temp_dir().join(format!("test_forge_chain_{}.db", std::process::id()));
        let store = crate::chain::workflow_store::WorkflowStore::open(&tmp).unwrap();

        // Use requirement keywords that match the shopify_dropship template.
        let result = SkillForge::forge_chain(
            "shopify order dropship",
            "My Order Skill",
            &generator,
            &templates,
            &store,
        );

        assert!(
            result.is_ok(),
            "forge_chain should succeed: {:?}",
            result.err()
        );
        let forged = result.unwrap();
        assert_eq!(forged.name, "My Order Skill");
        assert_eq!(forged.tier, SkillTier::Chain);
        assert_eq!(forged.workflow_id, Some("my_order_skill".to_string()));
        assert!(forged.wasm_path.is_none());
        assert!(forged.script_path.is_none());

        // Verify the workflow was stored.
        let loaded = store.get_def("my_order_skill").unwrap();
        assert!(loaded.is_some(), "Stored workflow should be retrievable");
        let def = loaded.unwrap();
        assert_eq!(def.name, "My Order Skill");

        // Clean up.
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_forge_wasm_not_implemented() {
        let result = SkillForge::forge_wasm("do something", "test_skill", Path::new("/tmp"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Level 1 (TOTP)"),
            "Error should mention TOTP requirement, got: {}",
            err
        );
        assert!(
            err.contains("not yet fully implemented"),
            "Error should mention not implemented, got: {}",
            err
        );
    }

    #[test]
    fn test_forge_shell_returns_script() {
        let scripts_dir = Path::new("/tmp/test_scripts");
        let result =
            SkillForge::forge_shell("list files in a directory", "File Lister", scripts_dir);

        assert!(
            result.is_ok(),
            "forge_shell should succeed: {:?}",
            result.err()
        );
        let (forged, script_content) = result.unwrap();

        assert_eq!(forged.name, "File Lister");
        assert_eq!(forged.tier, SkillTier::Shell);
        assert!(forged.workflow_id.is_none());
        assert!(forged.wasm_path.is_none());
        assert_eq!(
            forged.script_path,
            Some("/tmp/test_scripts/file_lister.sh".to_string())
        );

        // Verify script content.
        assert!(script_content.starts_with("#!/bin/bash\n"));
        assert!(script_content.contains("# Skill: File Lister"));
        assert!(script_content.contains("# Requirement: list files in a directory"));
        assert!(script_content.contains("echo 'Skill not yet implemented"));
    }

    #[test]
    fn test_skill_tier_display() {
        assert_eq!(format!("{}", SkillTier::Chain), "Workflow");
        assert_eq!(format!("{}", SkillTier::Wasm), "Wasm");
        assert_eq!(format!("{}", SkillTier::Shell), "Shell");
    }
}
