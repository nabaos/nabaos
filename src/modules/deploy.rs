//! Docker Compose generation based on module profile.

use super::profile::ModuleProfile;

/// Generate a docker-compose.yml string based on the enabled modules.
pub fn generate_docker_compose(profile: &ModuleProfile) -> String {
    let mut services = Vec::new();

    // Core service (always present)
    let mut core_svc = String::from(
        "  nyaya-core:
    image: nabaos:latest
    build: .
    restart: unless-stopped
    volumes:
      - nyaya-data:/data
    environment:
      - NABA_DATA_DIR=/data",
    );

    if profile.web {
        core_svc.push_str("\n      - NABA_WEB_BIND=0.0.0.0:8919");
        core_svc.push_str("\n    ports:\n      - \"8919:8919\"");
    }

    if profile.telegram {
        core_svc.push_str("\n      - NABA_TELEGRAM_BOT_TOKEN=${NABA_TELEGRAM_BOT_TOKEN}");
    }

    services.push(core_svc);

    if profile.browser {
        services.push(String::from(
            "  nyaya-browser:
    image: chromedp/headless-shell:latest
    restart: unless-stopped
    ports:
      - \"9222:9222\"",
        ));
    }

    let mut compose = String::from("version: '3.8'\n\nservices:\n");
    for svc in &services {
        compose.push_str(svc);
        compose.push('\n');
    }
    compose.push_str("\nvolumes:\n  nyaya-data:\n");

    compose
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_compose_minimal() {
        let mut profile = ModuleProfile::default();
        profile.web = false;
        profile.browser = false;
        let compose = generate_docker_compose(&profile);
        assert!(compose.contains("nyaya-core"));
        assert!(!compose.contains("nyaya-browser"));
    }

    #[test]
    fn test_generate_compose_with_web_and_browser() {
        let mut profile = ModuleProfile::default();
        profile.web = true;
        profile.browser = true;
        let compose = generate_docker_compose(&profile);
        assert!(compose.contains("nyaya-core"));
        assert!(compose.contains("8919:8919"));
        assert!(compose.contains("nyaya-browser"));
        assert!(compose.contains("chromedp"));
    }

    #[test]
    fn test_generate_compose_valid_yaml() {
        let profile = ModuleProfile::default();
        let compose = generate_docker_compose(&profile);
        // Should be parseable as YAML
        let _: serde_yaml::Value = serde_yaml::from_str(&compose).unwrap();
    }
}
