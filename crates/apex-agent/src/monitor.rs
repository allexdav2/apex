use std::collections::VecDeque;

pub struct CoverageMonitor {
    window: VecDeque<(u64, usize)>,
    window_size: usize,
    stall_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorAction {
    Normal,
    SwitchStrategy,
    AgentCycle,
    Stop,
}

impl CoverageMonitor {
    pub fn new(window_size: usize) -> Self {
        CoverageMonitor {
            window: VecDeque::new(),
            window_size,
            stall_count: 0,
        }
    }

    pub fn record(&mut self, iteration: u64, covered: usize) {
        // Check if coverage grew compared to most recent sample.
        let grew = self.window.back().is_some_and(|&(_, prev)| covered > prev);

        if grew {
            self.stall_count = 0;
        } else if !self.window.is_empty() {
            self.stall_count += 1;
        }

        self.window.push_back((iteration, covered));
        if self.window.len() > self.window_size {
            self.window.pop_front();
        }
    }

    pub fn growth_rate(&self) -> f64 {
        if self.window.len() < 2 {
            return 0.0;
        }
        let oldest = self.window.front().map(|e| e.1).unwrap_or(0);
        let newest = self.window.back().map(|e| e.1).unwrap_or(0);
        (newest as f64 - oldest as f64) / self.window.len() as f64
    }

    pub fn action(&self) -> MonitorAction {
        if self.stall_count == 0 {
            MonitorAction::Normal
        } else if self.stall_count < 2 * self.window_size {
            MonitorAction::SwitchStrategy
        } else if self.stall_count < 4 * self.window_size {
            MonitorAction::AgentCycle
        } else {
            MonitorAction::Stop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_window() {
        let m = CoverageMonitor::new(5);
        assert_eq!(m.growth_rate(), 0.0);
    }

    #[test]
    fn record_single_sample() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        assert_eq!(m.growth_rate(), 0.0);
    }

    #[test]
    fn record_growing_coverage() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        m.record(1, 20);
        m.record(2, 30);
        assert!(m.growth_rate() > 0.0);
        assert_eq!(m.action(), MonitorAction::Normal);
    }

    #[test]
    fn stalled_coverage_escalates() {
        let mut m = CoverageMonitor::new(5);
        m.record(0, 10);
        for i in 1..=10 {
            m.record(i, 10);
        }
        assert_ne!(m.action(), MonitorAction::Normal);
    }

    #[test]
    fn window_evicts_old_entries() {
        let mut m = CoverageMonitor::new(3);
        for i in 0..5 {
            m.record(i as u64, i * 10);
        }
        assert_eq!(m.window.len(), 3);
    }

    #[test]
    fn action_escalation_levels() {
        let mut m = CoverageMonitor::new(3);
        m.record(0, 10);

        // 3 stalls → SwitchStrategy (stall_count < 2*3=6)
        for i in 1..=3 {
            m.record(i, 10);
        }
        assert_eq!(m.stall_count, 3);
        assert_eq!(m.action(), MonitorAction::SwitchStrategy);

        // 6 stalls → AgentCycle (stall_count >= 2*3=6, < 4*3=12)
        for i in 4..=6 {
            m.record(i, 10);
        }
        assert_eq!(m.stall_count, 6);
        assert_eq!(m.action(), MonitorAction::AgentCycle);

        // 12 stalls → Stop (stall_count >= 4*3=12)
        for i in 7..=12 {
            m.record(i as u64, 10);
        }
        assert_eq!(m.stall_count, 12);
        assert_eq!(m.action(), MonitorAction::Stop);
    }

    #[test]
    fn recovery_resets_escalation() {
        let mut m = CoverageMonitor::new(3);
        m.record(0, 10);
        // Stall a few times
        for i in 1..=5 {
            m.record(i, 10);
        }
        assert_ne!(m.action(), MonitorAction::Normal);

        // Now grow — should reset
        m.record(6, 20);
        assert_eq!(m.action(), MonitorAction::Normal);
    }
}
