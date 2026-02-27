use crate::pea::objective::BudgetStrategy;

// ---------------------------------------------------------------------------
// BudgetMode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum BudgetMode {
    Aggressive,
    Conservative,
    Minimal,
    Exhausted,
}

// ---------------------------------------------------------------------------
// BudgetController
// ---------------------------------------------------------------------------

pub struct BudgetController {
    budget_usd: f64,
    spent_usd: f64,
    strategy: BudgetStrategy,
}

impl BudgetController {
    pub fn new(budget_usd: f64, spent_usd: f64, strategy: BudgetStrategy) -> Self {
        Self {
            budget_usd,
            spent_usd,
            strategy,
        }
    }

    pub fn record_spend(&mut self, amount_usd: f64) {
        self.spent_usd += amount_usd;
    }

    pub fn spent(&self) -> f64 {
        self.spent_usd
    }

    pub fn remaining(&self) -> f64 {
        (self.budget_usd - self.spent_usd).max(0.0)
    }

    pub fn utilization(&self) -> f64 {
        if self.budget_usd <= 0.0 {
            return 1.0;
        }
        self.spent_usd / self.budget_usd
    }

    pub fn can_afford(&self, estimated_cost_usd: f64) -> bool {
        self.remaining() >= estimated_cost_usd
    }

    pub fn current_mode(&self) -> BudgetMode {
        let u = self.utilization();
        match self.strategy {
            BudgetStrategy::Aggressive => {
                if u >= 1.0 {
                    BudgetMode::Exhausted
                } else if u >= 0.95 {
                    BudgetMode::Minimal
                } else {
                    BudgetMode::Aggressive
                }
            }
            BudgetStrategy::Adaptive => {
                if u >= 1.0 {
                    BudgetMode::Exhausted
                } else if u >= 0.95 {
                    BudgetMode::Minimal
                } else if u >= 0.80 {
                    BudgetMode::Conservative
                } else {
                    BudgetMode::Aggressive
                }
            }
            BudgetStrategy::Conservative => {
                if u >= 1.0 {
                    BudgetMode::Exhausted
                } else if u >= 0.90 {
                    BudgetMode::Minimal
                } else {
                    BudgetMode::Conservative
                }
            }
            BudgetStrategy::Minimal => BudgetMode::Minimal,
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
    fn test_budget_aggressive_mode_at_50_percent() {
        let ctrl = BudgetController::new(100.0, 50.0, BudgetStrategy::Aggressive);
        assert_eq!(ctrl.current_mode(), BudgetMode::Aggressive);
    }

    #[test]
    fn test_budget_adaptive_switches_conservative_at_80() {
        let ctrl = BudgetController::new(100.0, 80.0, BudgetStrategy::Adaptive);
        assert_eq!(ctrl.current_mode(), BudgetMode::Conservative);
    }

    #[test]
    fn test_budget_exhausted_at_100_percent() {
        let ctrl = BudgetController::new(100.0, 100.0, BudgetStrategy::Aggressive);
        assert_eq!(ctrl.current_mode(), BudgetMode::Exhausted);
    }

    #[test]
    fn test_budget_can_afford() {
        let ctrl = BudgetController::new(100.0, 80.0, BudgetStrategy::Adaptive);
        assert!(ctrl.can_afford(15.0));
        assert!(!ctrl.can_afford(25.0));
    }
}
