use super::registry::{ApiFormat, ModelDef, ProviderDef};

/// Helper to create an OpenAI-compatible provider with no predefined models.
fn openai_compat(id: &str, name: &str, base_url: &str) -> ProviderDef {
    ProviderDef {
        id: id.to_string(),
        display_name: name.to_string(),
        api_format: ApiFormat::OpenAI,
        base_url: base_url.to_string(),
        models: Vec::new(),
        default_model: String::new(),
        supports_tools: true,
        supports_vision: false,
        supports_structured_output: false, // conservative default
    }
}

/// Returns all 61 built-in provider definitions.
#[allow(clippy::vec_init_then_push)]
pub fn builtin_providers() -> Vec<ProviderDef> {
    let mut providers = Vec::with_capacity(61);

    // ── Big 5 (with full model definitions) ──────────────────────────

    providers.push(ProviderDef {
        id: "anthropic".to_string(),
        display_name: "Anthropic".to_string(),
        api_format: ApiFormat::Anthropic,
        base_url: "https://api.anthropic.com".to_string(),
        models: vec![
            ModelDef {
                id: "claude-opus-4-6".to_string(),
                display_name: "Claude Opus 4.6".to_string(),
                context_window: 200_000,
                input_price_per_mtok: 15.0,
                output_price_per_mtok: 75.0,
            },
            ModelDef {
                id: "claude-sonnet-4-6".to_string(),
                display_name: "Claude Sonnet 4.6".to_string(),
                context_window: 200_000,
                input_price_per_mtok: 3.0,
                output_price_per_mtok: 15.0,
            },
            ModelDef {
                id: "claude-haiku-4-5".to_string(),
                display_name: "Claude Haiku 4.5".to_string(),
                context_window: 200_000,
                input_price_per_mtok: 0.80,
                output_price_per_mtok: 4.0,
            },
        ],
        default_model: "claude-sonnet-4-6".to_string(),
        supports_tools: true,
        supports_vision: true,
        supports_structured_output: true,
    });

    providers.push(ProviderDef {
        id: "openai".to_string(),
        display_name: "OpenAI".to_string(),
        api_format: ApiFormat::OpenAI,
        base_url: "https://api.openai.com".to_string(),
        models: vec![
            ModelDef {
                id: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                context_window: 128_000,
                input_price_per_mtok: 2.50,
                output_price_per_mtok: 10.0,
            },
            ModelDef {
                id: "gpt-4o-mini".to_string(),
                display_name: "GPT-4o Mini".to_string(),
                context_window: 128_000,
                input_price_per_mtok: 0.15,
                output_price_per_mtok: 0.60,
            },
            ModelDef {
                id: "o3-mini".to_string(),
                display_name: "o3-mini".to_string(),
                context_window: 200_000,
                input_price_per_mtok: 1.10,
                output_price_per_mtok: 4.40,
            },
        ],
        default_model: "gpt-4o".to_string(),
        supports_tools: true,
        supports_vision: true,
        supports_structured_output: true,
    });

    providers.push(ProviderDef {
        id: "google".to_string(),
        display_name: "Google".to_string(),
        api_format: ApiFormat::Google,
        base_url: "https://generativelanguage.googleapis.com".to_string(),
        models: vec![
            ModelDef {
                id: "gemini-2.0-flash".to_string(),
                display_name: "Gemini 2.0 Flash".to_string(),
                context_window: 1_000_000,
                input_price_per_mtok: 0.10,
                output_price_per_mtok: 0.40,
            },
            ModelDef {
                id: "gemini-2.0-pro".to_string(),
                display_name: "Gemini 2.0 Pro".to_string(),
                context_window: 1_000_000,
                input_price_per_mtok: 1.25,
                output_price_per_mtok: 10.0,
            },
        ],
        default_model: "gemini-2.0-flash".to_string(),
        supports_tools: true,
        supports_vision: true,
        supports_structured_output: true,
    });

    providers.push(ProviderDef {
        id: "bedrock".to_string(),
        display_name: "AWS Bedrock".to_string(),
        api_format: ApiFormat::Bedrock,
        base_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
        models: vec![ModelDef {
            id: "anthropic.claude-sonnet-4-6-v1".to_string(),
            display_name: "Claude Sonnet 4.6 (Bedrock)".to_string(),
            context_window: 200_000,
            input_price_per_mtok: 3.0,
            output_price_per_mtok: 15.0,
        }],
        default_model: "anthropic.claude-sonnet-4-6-v1".to_string(),
        supports_tools: true,
        supports_vision: true,
        supports_structured_output: false,
    });

    providers.push(ProviderDef {
        id: "azure_openai".to_string(),
        display_name: "Azure OpenAI".to_string(),
        api_format: ApiFormat::Azure,
        base_url: "https://your-resource.openai.azure.com".to_string(),
        models: vec![ModelDef {
            id: "gpt-4o".to_string(),
            display_name: "GPT-4o (Azure)".to_string(),
            context_window: 128_000,
            input_price_per_mtok: 2.50,
            output_price_per_mtok: 10.0,
        }],
        default_model: "gpt-4o".to_string(),
        supports_tools: true,
        supports_vision: true,
        supports_structured_output: false,
    });

    // ── 50 OpenAI-compatible aggregators ──────────────────────────────

    providers.push(openai_compat(
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
    ));
    providers.push(openai_compat(
        "together",
        "Together AI",
        "https://api.together.xyz",
    ));
    providers.push(openai_compat(
        "fireworks",
        "Fireworks AI",
        "https://api.fireworks.ai/inference",
    ));
    providers.push({
        let mut p = openai_compat("groq", "Groq", "https://api.groq.com/openai");
        p.supports_structured_output = true;
        p
    });
    providers.push({
        let mut p = openai_compat("deepseek", "DeepSeek", "https://api.deepseek.com");
        p.supports_structured_output = true;
        p
    });
    providers.push({
        let mut p = openai_compat("mistral", "Mistral AI", "https://api.mistral.ai");
        p.supports_structured_output = true;
        p
    });
    providers.push(openai_compat(
        "cerebras",
        "Cerebras",
        "https://api.cerebras.ai",
    ));
    providers.push(openai_compat(
        "nanogpt",
        "NanoGPT",
        "https://nano-gpt.com/api/v1",
    ));
    providers.push(openai_compat(
        "featherless",
        "Featherless AI",
        "https://api.featherless.ai",
    ));
    providers.push(openai_compat(
        "perplexity",
        "Perplexity",
        "https://api.perplexity.ai",
    ));
    providers.push(openai_compat(
        "replicate",
        "Replicate",
        "https://api.replicate.com",
    ));
    providers.push(openai_compat(
        "deepinfra",
        "DeepInfra",
        "https://api.deepinfra.com",
    ));
    providers.push(openai_compat(
        "lepton",
        "Lepton AI",
        "https://api.lepton.ai",
    ));
    providers.push(openai_compat(
        "anyscale",
        "Anyscale",
        "https://api.anyscale.com",
    ));
    providers.push(openai_compat("cohere", "Cohere", "https://api.cohere.ai"));
    providers.push(openai_compat("ai21", "AI21 Labs", "https://api.ai21.com"));
    providers.push(openai_compat(
        "baseten",
        "Baseten",
        "https://api.baseten.co",
    ));
    providers.push(openai_compat("modal", "Modal", "https://api.modal.com"));
    providers.push(openai_compat("runpod", "RunPod", "https://api.runpod.ai"));
    providers.push(openai_compat("lambda", "Lambda", "https://api.lambda.chat"));
    providers.push(openai_compat("nebius", "Nebius", "https://api.nebius.ai"));
    providers.push(openai_compat(
        "novita",
        "Novita AI",
        "https://api.novita.ai/openai/v1",
    ));
    providers.push(openai_compat(
        "sambanova",
        "SambaNova",
        "https://api.sambanova.ai",
    ));
    providers.push(openai_compat(
        "octoai",
        "OctoAI",
        "https://api.octoai.cloud",
    ));
    providers.push(openai_compat(
        "hyperbolic",
        "Hyperbolic",
        "https://api.hyperbolic.xyz",
    ));
    providers.push(openai_compat(
        "cloudflare",
        "Cloudflare Workers AI",
        "https://api.cloudflare.com/client/v4/accounts",
    ));
    providers.push(openai_compat(
        "huggingface",
        "Hugging Face",
        "https://api-inference.huggingface.co",
    ));
    providers.push(openai_compat("ollama", "Ollama", "http://localhost:11434"));
    providers.push(openai_compat(
        "lmstudio",
        "LM Studio",
        "http://localhost:1234",
    ));
    providers.push(openai_compat("jan", "Jan", "http://localhost:1337"));
    providers.push(openai_compat("gpt4all", "GPT4All", "http://localhost:4891"));
    providers.push(openai_compat(
        "llamacpp",
        "llama.cpp",
        "http://localhost:8080",
    ));
    providers.push(openai_compat(
        "koboldcpp",
        "KoboldCpp",
        "http://localhost:5001",
    ));
    providers.push(openai_compat(
        "tabbyapi",
        "TabbyAPI",
        "http://localhost:5000",
    ));
    providers.push(openai_compat(
        "aphrodite",
        "Aphrodite",
        "http://localhost:2242",
    ));
    providers.push(openai_compat(
        "exllamav2",
        "ExLlamaV2",
        "http://localhost:5000",
    ));
    providers.push(openai_compat(
        "xinference",
        "Xinference",
        "http://localhost:9997",
    ));
    providers.push(openai_compat("localai", "LocalAI", "http://localhost:8080"));
    providers.push(openai_compat("litellm", "LiteLLM", "http://localhost:4000"));
    providers.push(openai_compat(
        "portkey",
        "Portkey",
        "https://api.portkey.ai",
    ));
    providers.push(openai_compat(
        "helicone",
        "Helicone",
        "https://gateway.helicone.ai",
    ));
    providers.push(openai_compat(
        "martian",
        "Martian",
        "https://api.withmartian.com",
    ));
    providers.push(openai_compat(
        "not_diamond",
        "Not Diamond",
        "https://api.notdiamond.ai",
    ));
    providers.push(openai_compat("unify", "Unify", "https://api.unify.ai"));
    providers.push(openai_compat(
        "braintrust",
        "Braintrust",
        "https://api.braintrust.dev",
    ));
    providers.push(openai_compat("glhf", "GLHF", "https://glhf.chat/api"));
    providers.push(openai_compat(
        "kluster",
        "Kluster",
        "https://api.kluster.ai",
    ));
    providers.push(openai_compat(
        "infermatic",
        "Infermatic",
        "https://api.infermatic.ai",
    ));
    providers.push(openai_compat("chutes", "Chutes", "https://api.chutes.ai"));
    providers.push(openai_compat(
        "parasail",
        "Parasail",
        "https://api.parasail.io",
    ));
    providers.push(openai_compat(
        "qwen",
        "Qwen (DashScope)",
        "https://dashscope.aliyuncs.com/compatible-mode",
    ));
    providers.push(openai_compat(
        "kimi",
        "Kimi (Moonshot AI)",
        "https://api.moonshot.cn",
    ));
    providers.push(openai_compat(
        "baichuan",
        "Baichuan",
        "https://api.baichuan-ai.com",
    ));
    providers.push(openai_compat("yi", "Yi (01.AI)", "https://api.01.ai"));
    providers.push(openai_compat(
        "zhipu",
        "Zhipu (GLM)",
        "https://open.bigmodel.cn/api/paas",
    ));
    providers.push(openai_compat(
        "minimax",
        "MiniMax",
        "https://api.minimax.chat",
    ));

    providers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_providers_count() {
        let providers = builtin_providers();
        assert_eq!(
            providers.len(),
            61,
            "Expected exactly 61 built-in providers"
        );
    }

    #[test]
    fn test_openai_supports_structured_output() {
        let providers = builtin_providers();
        let openai = providers.iter().find(|p| p.id == "openai").unwrap();
        assert!(openai.supports_structured_output);
    }

    #[test]
    fn test_generic_provider_no_structured_output() {
        let providers = builtin_providers();
        let together = providers.iter().find(|p| p.id == "together").unwrap();
        assert!(!together.supports_structured_output);
    }

    #[test]
    fn test_big5_have_models() {
        let providers = builtin_providers();
        let big5 = ["anthropic", "openai", "google", "bedrock", "azure_openai"];
        for id in &big5 {
            let p = providers
                .iter()
                .find(|p| p.id == *id)
                .unwrap_or_else(|| panic!("Missing provider: {}", id));
            assert!(!p.models.is_empty(), "Provider {} should have models", id);
            assert!(
                !p.default_model.is_empty(),
                "Provider {} should have a default model",
                id
            );
        }
    }
}
