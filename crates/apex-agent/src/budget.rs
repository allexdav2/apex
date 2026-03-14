//! Budget allocator — distributes iteration budgets across strategies.
//!
//! Tracks per-strategy effectiveness (branches found) and allocates
//! proportionally, with a minimum share floor so no strategy starves.

/// Allocates iteration budgets across N strategies.
#[derive(Debug, Clone)]
pub struct BudgetAllocator {
    total_budget: u64,
    num_strategies: usize,
    effectiveness: Vec<u64>,
    minimum_share: f64,
}

impl BudgetAllocator {
    pub fn new(total_budget: u64, num_strategies: usize) -> Self {
        BudgetAllocator {
            total_budget,
            num_strategies,
            effectiveness: vec![0; num_strategies],
            minimum_share: 0.05,
        }
    }

    /// Report that strategy `index` discovered `new_branches` branches.
    pub fn report(&mut self, index: usize, new_branches: u64) {
        if index < self.num_strategies {
            self.effectiveness[index] += new_branches;
        }
    }

    /// Set the minimum share each strategy receives (0.0 to 1.0).
    pub fn set_minimum_share(&mut self, share: f64) {
        self.minimum_share = share.clamp(0.0, 1.0 / self.num_strategies as f64);
    }

    /// Allocate budgets proportional to effectiveness.
    pub fn allocate(&self) -> Vec<u64> {
        let n = self.num_strategies;
        let total_eff: u64 = self.effectiveness.iter().sum();

        if total_eff == 0 {
            // Equal split when no data.
            let per = self.total_budget / n as u64;
            let mut budgets = vec![per; n];
            // Distribute remainder.
            let remainder = self.total_budget - per * n as u64;
            for budget in budgets.iter_mut().take(remainder as usize) {
                *budget += 1;
            }
            return budgets;
        }

        let min_budget = (self.total_budget as f64 * self.minimum_share) as u64;
        let reserved = min_budget * n as u64;
        let distributable = self.total_budget.saturating_sub(reserved);

        let mut budgets: Vec<u64> = self
            .effectiveness
            .iter()
            .map(|&eff| {
                let share = eff as f64 / total_eff as f64;
                min_budget + (distributable as f64 * share) as u64
            })
            .collect();

        // Adjust rounding errors.
        let allocated: u64 = budgets.iter().sum();
        if allocated < self.total_budget {
            budgets[0] += self.total_budget - allocated;
        }

        budgets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_allocation_with_no_history() {
        let allocator = BudgetAllocator::new(1000, 3);
        let budgets = allocator.allocate();
        // With no performance data, split evenly.
        assert_eq!(budgets.len(), 3);
        let total: u64 = budgets.iter().sum();
        assert_eq!(total, 1000);
    }

    #[test]
    fn report_adjusts_allocation() {
        let mut allocator = BudgetAllocator::new(1000, 2);
        // Strategy 0 found 10 branches, strategy 1 found 0.
        allocator.report(0, 10);
        allocator.report(1, 0);
        let budgets = allocator.allocate();
        // Strategy 0 should get more budget.
        assert!(budgets[0] > budgets[1]);
    }

    #[test]
    fn minimum_budget_guaranteed() {
        let mut allocator = BudgetAllocator::new(100, 2);
        allocator.set_minimum_share(0.1);
        allocator.report(0, 100);
        allocator.report(1, 0);
        let budgets = allocator.allocate();
        // Even strategy 1 gets at least 10%.
        assert!(budgets[1] >= 10);
    }

    #[test]
    fn single_strategy_gets_full_budget() {
        let allocator = BudgetAllocator::new(500, 1);
        let budgets = allocator.allocate();
        assert_eq!(budgets, vec![500]);
    }
}
