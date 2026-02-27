//! Circuit breaker engine — safety rules that can halt chain execution.
//!
//! Parsed from B: lines in <nyaya> blocks. Examples:
//!   B:amount>1000|abort|"Transaction exceeds $1000 limit"
//!   B:frequency>10/1h|throttle|"Too many requests per hour"
//!   B:ability:email.send|confirm|"Email send requires confirmation"
//!
//! Three actions: abort (stop chain), confirm (ask user), throttle (rate limit).

use std::collections::HashMap;

use crate::core::error::{NyayaError, Result};

/// A circuit breaker rule.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Unique rule ID
    pub id: String,
    /// The chain this breaker applies to (or "*" for global)
    pub chain_id: String,
    /// The condition that triggers the breaker
    pub condition: BreakerCondition,
    /// What to do when triggered
    pub action: BreakerAction,
    /// Human-readable reason
    pub reason: String,
}

/// Condition that triggers a circuit breaker.
#[derive(Debug, Clone)]
pub enum BreakerCondition {
    /// A numeric output exceeds a threshold: amount>1000
    ThresholdExceeded { key: String, threshold: f64 },
    /// Execution frequency exceeds a rate: frequency>10/1h
    FrequencyExceeded { max_count: u32, window_secs: u64 },
    /// A specific ability is being called: ability:email.send
    AbilityUsed { ability: String },
    /// An output contains a specific pattern
    OutputContains { key: String, pattern: String },
}

/// What to do when a circuit breaker fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerAction {
    /// Stop the chain immediately
    Abort,
    /// Ask the user for confirmation before proceeding
    Confirm,
    /// Rate-limit (delay or skip)
    Throttle,
}

/// Result of evaluating circuit breakers.
#[derive(Debug)]
pub struct BreakerCheck {
    /// Whether execution should proceed
    pub proceed: bool,
    /// Which breakers fired (if any)
    pub fired: Vec<FiredBreaker>,
}

#[derive(Debug)]
pub struct FiredBreaker {
    pub breaker_id: String,
    pub action: BreakerAction,
    pub reason: String,
}

/// Circuit breaker registry with execution history for frequency tracking.
pub struct BreakerRegistry {
    breakers: Vec<CircuitBreaker>,
    /// Execution timestamps per chain: chain_id → Vec<unix_timestamp_secs>
    /// Used for FrequencyExceeded condition (sliding window counter).
    execution_history: std::sync::Mutex<HashMap<String, Vec<u64>>>,
}

impl Default for BreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: Vec::new(),
            execution_history: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Record an execution event for frequency tracking.
    pub fn record_execution(&self, chain_id: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Ok(mut history) = self.execution_history.lock() {
            let timestamps = history.entry(chain_id.to_string()).or_insert_with(Vec::new);
            timestamps.push(now);
            // Cap history at 10000 entries to prevent unbounded growth
            if timestamps.len() > 10000 {
                timestamps.drain(..timestamps.len() - 10000);
            }
        }
    }

    /// Register a circuit breaker.
    pub fn register(&mut self, breaker: CircuitBreaker) {
        self.breakers.push(breaker);
    }

    /// Parse and register breakers from B: lines in a <nyaya> block.
    /// Format: B:condition|action|"reason"
    pub fn register_from_spec(&mut self, chain_id: &str, spec: &str) -> Result<()> {
        let breaker = parse_breaker_spec(chain_id, spec)?;
        self.register(breaker);
        Ok(())
    }

    /// Evaluate all breakers for a given chain execution context.
    /// `step_outputs` contains the current chain outputs.
    /// `ability` is the next ability about to be called.
    pub fn evaluate(
        &self,
        chain_id: &str,
        step_outputs: &HashMap<String, String>,
        ability: &str,
    ) -> BreakerCheck {
        let mut fired = Vec::new();

        for breaker in &self.breakers {
            if breaker.chain_id != "*" && breaker.chain_id != chain_id {
                continue;
            }

            let triggered = match &breaker.condition {
                BreakerCondition::ThresholdExceeded { key, threshold } => step_outputs
                    .get(key)
                    .and_then(|v| v.parse::<f64>().ok())
                    .map_or(true, |val| val > *threshold),

                BreakerCondition::AbilityUsed {
                    ability: target_ability,
                } => ability == target_ability,

                BreakerCondition::OutputContains { key, pattern } => {
                    step_outputs.get(key).is_some_and(|v| v.contains(pattern))
                }

                BreakerCondition::FrequencyExceeded {
                    max_count,
                    window_secs,
                } => {
                    // H7: Sliding window frequency check using execution history
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let window_start = now.saturating_sub(*window_secs);
                    if let Ok(history) = self.execution_history.lock() {
                        history.get(chain_id).is_some_and(|timestamps| {
                            let count = timestamps.iter().filter(|&&ts| ts >= window_start).count();
                            count >= *max_count as usize
                        })
                    } else {
                        false // Can't check — allow through
                    }
                }
            };

            if triggered {
                fired.push(FiredBreaker {
                    breaker_id: breaker.id.clone(),
                    action: breaker.action,
                    reason: breaker.reason.clone(),
                });
            }
        }

        // SECURITY: Both Abort AND Confirm actions halt execution.
        // Confirm should require human approval, but since no interactive
        // confirmation channel is available at this level, treat it as Abort
        // to prevent silent bypass of safety-critical checks.
        let proceed = !fired
            .iter()
            .any(|f| f.action == BreakerAction::Abort || f.action == BreakerAction::Confirm);

        BreakerCheck { proceed, fired }
    }

    /// Get all registered breakers for a chain.
    pub fn breakers_for(&self, chain_id: &str) -> Vec<&CircuitBreaker> {
        self.breakers
            .iter()
            .filter(|b| b.chain_id == "*" || b.chain_id == chain_id)
            .collect()
    }
}

/// Parse a B: line spec into a CircuitBreaker.
/// Format examples:
///   amount>1000|abort|"Transaction exceeds limit"
///   ability:email.send|confirm|"Requires confirmation"
///   output:error_msg|contains:fail|abort|"Step produced failure"
fn parse_breaker_spec(chain_id: &str, spec: &str) -> Result<CircuitBreaker> {
    let parts: Vec<&str> = spec.splitn(3, '|').collect();
    if parts.len() < 2 {
        return Err(NyayaError::Config(format!(
            "Invalid breaker spec (need condition|action): {}",
            spec
        )));
    }

    let condition_str = parts[0].trim();
    let action_str = parts[1].trim();
    let reason = parts
        .get(2)
        .map(|r| r.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| format!("Circuit breaker: {}", condition_str));

    let condition = if let Some(rest) = condition_str.strip_prefix("ability:") {
        BreakerCondition::AbilityUsed {
            ability: rest.to_string(),
        }
    } else if let Some(rest) = condition_str.strip_prefix("frequency>") {
        // Parse "10/1h" format
        let fparts: Vec<&str> = rest.splitn(2, '/').collect();
        let max_count = fparts[0]
            .parse::<u32>()
            .map_err(|_| NyayaError::Config("Invalid frequency count".into()))?;
        let window_secs = parse_duration(fparts.get(1).unwrap_or(&"1h"))?;
        BreakerCondition::FrequencyExceeded {
            max_count,
            window_secs,
        }
    } else if condition_str.contains('>') {
        // Generic threshold: key>value
        let kv: Vec<&str> = condition_str.splitn(2, '>').collect();
        let key = kv[0].to_string();
        let threshold = kv[1]
            .parse::<f64>()
            .map_err(|_| NyayaError::Config(format!("Invalid threshold: {}", kv[1])))?;
        BreakerCondition::ThresholdExceeded { key, threshold }
    } else {
        return Err(NyayaError::Config(format!(
            "Unknown breaker condition: {}",
            condition_str
        )));
    };

    let action = match action_str {
        "abort" => BreakerAction::Abort,
        "confirm" => BreakerAction::Confirm,
        "throttle" => BreakerAction::Throttle,
        _ => {
            return Err(NyayaError::Config(format!(
                "Unknown breaker action: {}",
                action_str
            )))
        }
    };

    let id = format!("{}_{}", chain_id, condition_str.replace('>', "_gt_"));

    Ok(CircuitBreaker {
        id,
        chain_id: chain_id.to_string(),
        condition,
        action,
        reason,
    })
}

/// Parse a duration string like "1h", "30m", "1d" into seconds.
fn parse_duration(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(3600); // default 1 hour
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str
        .parse()
        .map_err(|_| NyayaError::Config(format!("Invalid duration number: {}", num_str)))?;

    match unit {
        "s" => Ok(num),
        "m" => Ok(num * 60),
        "h" => Ok(num * 3600),
        "d" => Ok(num * 86400),
        _ => Err(NyayaError::Config(format!(
            "Unknown duration unit: {}",
            unit
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_threshold_breaker() {
        let breaker =
            parse_breaker_spec("trade_chain", "amount>1000|abort|\"Exceeds limit\"").unwrap();
        assert_eq!(breaker.chain_id, "trade_chain");
        assert_eq!(breaker.action, BreakerAction::Abort);
        assert!(matches!(
            breaker.condition,
            BreakerCondition::ThresholdExceeded { threshold, .. } if threshold == 1000.0
        ));
    }

    #[test]
    fn test_parse_ability_breaker() {
        let breaker =
            parse_breaker_spec("*", "ability:email.send|confirm|\"Requires confirmation\"")
                .unwrap();
        assert_eq!(breaker.chain_id, "*");
        assert_eq!(breaker.action, BreakerAction::Confirm);
        assert!(matches!(
            breaker.condition,
            BreakerCondition::AbilityUsed { ref ability } if ability == "email.send"
        ));
    }

    #[test]
    fn test_parse_frequency_breaker() {
        let breaker =
            parse_breaker_spec("poll_chain", "frequency>10/1h|throttle|\"Rate limited\"").unwrap();
        assert!(matches!(
            breaker.condition,
            BreakerCondition::FrequencyExceeded {
                max_count: 10,
                window_secs: 3600,
            }
        ));
    }

    #[test]
    fn test_evaluate_threshold_fires() {
        let mut reg = BreakerRegistry::new();
        reg.register_from_spec("trade", "amount>500|abort|\"Too high\"")
            .unwrap();

        let outputs = HashMap::from([("amount".into(), "750".into())]);
        let check = reg.evaluate("trade", &outputs, "trading.execute");

        assert!(!check.proceed);
        assert_eq!(check.fired.len(), 1);
        assert_eq!(check.fired[0].action, BreakerAction::Abort);
    }

    #[test]
    fn test_evaluate_threshold_passes() {
        let mut reg = BreakerRegistry::new();
        reg.register_from_spec("trade", "amount>500|abort|\"Too high\"")
            .unwrap();

        let outputs = HashMap::from([("amount".into(), "200".into())]);
        let check = reg.evaluate("trade", &outputs, "trading.execute");

        assert!(check.proceed);
        assert!(check.fired.is_empty());
    }

    #[test]
    fn test_evaluate_ability_breaker() {
        let mut reg = BreakerRegistry::new();
        reg.register_from_spec("*", "ability:email.send|confirm|\"Confirm email\"")
            .unwrap();

        let check = reg.evaluate("any_chain", &HashMap::new(), "email.send");
        assert!(!check.proceed); // C5: Confirm now blocks (no interactive confirmation available)
        assert_eq!(check.fired.len(), 1);
        assert_eq!(check.fired[0].action, BreakerAction::Confirm);
    }

    #[test]
    fn test_global_breaker_applies_to_all_chains() {
        let mut reg = BreakerRegistry::new();
        reg.register_from_spec("*", "ability:email.send|abort|\"No email\"")
            .unwrap();

        let check1 = reg.evaluate("chain_a", &HashMap::new(), "email.send");
        let check2 = reg.evaluate("chain_b", &HashMap::new(), "email.send");

        assert!(!check1.proceed);
        assert!(!check2.proceed);
    }

    #[test]
    fn test_chain_specific_breaker_ignores_other_chains() {
        let mut reg = BreakerRegistry::new();
        reg.register_from_spec("chain_a", "amount>100|abort|\"Limit\"")
            .unwrap();

        let outputs = HashMap::from([("amount".into(), "200".into())]);
        let check_a = reg.evaluate("chain_a", &outputs, "step");
        let check_b = reg.evaluate("chain_b", &outputs, "step");

        assert!(!check_a.proceed); // Fires for chain_a
        assert!(check_b.proceed); // Does NOT fire for chain_b
    }
}
