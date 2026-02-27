use serde::{Deserialize, Serialize};

/// A cost estimate for a collaboration pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub num_calls: u32,
    pub estimated_input_tokens: u32,
    pub estimated_output_tokens: u32,
    pub estimated_cost_usd: f64,
}

/// Estimate the cost of running a council collaboration.
///
/// Each participant produces one response per round, plus one synthesis call.
/// Estimated tokens: ~500 input + ~300 output per call.
pub fn estimate_council_cost(
    num_participants: u32,
    rounds: u32,
    avg_input_price: f64,
    avg_output_price: f64,
) -> CostEstimate {
    let num_calls = num_participants * rounds + 1; // +1 for synthesis
    let input_tokens_per_call: u32 = 500;
    let output_tokens_per_call: u32 = 300;
    let estimated_input_tokens = num_calls * input_tokens_per_call;
    let estimated_output_tokens = num_calls * output_tokens_per_call;
    let estimated_cost_usd = (estimated_input_tokens as f64 * avg_input_price
        + estimated_output_tokens as f64 * avg_output_price)
        / 1_000_000.0;

    CostEstimate {
        num_calls,
        estimated_input_tokens,
        estimated_output_tokens,
        estimated_cost_usd,
    }
}

/// Estimate the cost of running a relay pipeline.
///
/// Each stage produces one response.
/// Estimated tokens: ~500 input + ~500 output per stage.
pub fn estimate_relay_cost(
    num_stages: u32,
    avg_input_price: f64,
    avg_output_price: f64,
) -> CostEstimate {
    let num_calls = num_stages;
    let input_tokens_per_call: u32 = 500;
    let output_tokens_per_call: u32 = 500;
    let estimated_input_tokens = num_calls * input_tokens_per_call;
    let estimated_output_tokens = num_calls * output_tokens_per_call;
    let estimated_cost_usd = (estimated_input_tokens as f64 * avg_input_price
        + estimated_output_tokens as f64 * avg_output_price)
        / 1_000_000.0;

    CostEstimate {
        num_calls,
        estimated_input_tokens,
        estimated_output_tokens,
        estimated_cost_usd,
    }
}

/// Estimate the cost of running an ensemble collaboration.
///
/// Each agent produces one section, plus optionally an editor call.
/// Estimated tokens: ~800 input + ~800 output per agent (more context passing).
pub fn estimate_ensemble_cost(
    num_agents: u32,
    has_editor: bool,
    avg_input_price: f64,
    avg_output_price: f64,
) -> CostEstimate {
    let num_calls = if has_editor {
        num_agents + 1
    } else {
        num_agents
    };
    let input_tokens_per_call: u32 = 800;
    let output_tokens_per_call: u32 = 800;
    let estimated_input_tokens = num_calls * input_tokens_per_call;
    let estimated_output_tokens = num_calls * output_tokens_per_call;
    let estimated_cost_usd = (estimated_input_tokens as f64 * avg_input_price
        + estimated_output_tokens as f64 * avg_output_price)
        / 1_000_000.0;

    CostEstimate {
        num_calls,
        estimated_input_tokens,
        estimated_output_tokens,
        estimated_cost_usd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_council_cost() {
        // 3 participants, 2 rounds => 3*2 + 1 = 7 calls
        let estimate = estimate_council_cost(3, 2, 3.0, 15.0);
        assert_eq!(estimate.num_calls, 7);
        assert_eq!(estimate.estimated_input_tokens, 7 * 500);
        assert_eq!(estimate.estimated_output_tokens, 7 * 300);
        // Cost: (3500 * 3.0 + 2100 * 15.0) / 1_000_000
        let expected_cost = (3500.0 * 3.0 + 2100.0 * 15.0) / 1_000_000.0;
        assert!((estimate.estimated_cost_usd - expected_cost).abs() < 1e-10);
    }

    #[test]
    fn test_estimate_relay_cost() {
        // 4 stages
        let estimate = estimate_relay_cost(4, 3.0, 15.0);
        assert_eq!(estimate.num_calls, 4);
        assert_eq!(estimate.estimated_input_tokens, 4 * 500);
        assert_eq!(estimate.estimated_output_tokens, 4 * 500);
        let expected_cost = (2000.0 * 3.0 + 2000.0 * 15.0) / 1_000_000.0;
        assert!((estimate.estimated_cost_usd - expected_cost).abs() < 1e-10);
    }

    #[test]
    fn test_estimate_ensemble_cost() {
        // 3 agents with editor => 4 calls
        let estimate = estimate_ensemble_cost(3, true, 3.0, 15.0);
        assert_eq!(estimate.num_calls, 4);
        assert_eq!(estimate.estimated_input_tokens, 4 * 800);
        assert_eq!(estimate.estimated_output_tokens, 4 * 800);

        // 3 agents without editor => 3 calls
        let estimate_no_editor = estimate_ensemble_cost(3, false, 3.0, 15.0);
        assert_eq!(estimate_no_editor.num_calls, 3);
        assert_eq!(estimate_no_editor.estimated_input_tokens, 3 * 800);
        assert_eq!(estimate_no_editor.estimated_output_tokens, 3 * 800);

        // With editor should cost more
        assert!(estimate.estimated_cost_usd > estimate_no_editor.estimated_cost_usd);
    }
}
