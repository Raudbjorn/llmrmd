//! Reusable TUI widgets.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Gauge, Widget};

/// A token budget gauge that changes color based on usage.
///
/// - Green: < 50% used
/// - Yellow: 50-80% used
/// - Red: > 80% used
pub struct TokenGauge<'a> {
    used: usize,
    budget: usize,
    block: Option<Block<'a>>,
}

impl<'a> TokenGauge<'a> {
    pub fn new(used: usize, budget: usize) -> Self {
        Self {
            used,
            budget,
            block: None,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl Widget for TokenGauge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let ratio = if self.budget > 0 {
            (self.used as f64 / self.budget as f64).min(1.0)
        } else {
            0.0
        };

        let color = if ratio < 0.5 {
            Color::Green
        } else if ratio < 0.8 {
            Color::Yellow
        } else {
            Color::Red
        };

        let label = format!(
            "{}/{} tokens ({:.0}%)",
            self.used,
            self.budget,
            ratio * 100.0
        );

        let mut gauge = Gauge::default()
            .gauge_style(Style::default().fg(color).bg(Color::DarkGray))
            .ratio(ratio)
            .label(label);

        if let Some(block) = self.block {
            gauge = gauge.block(block);
        }

        gauge.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_ratio_clamped_to_one() {
        // Over budget should still render (ratio capped at 1.0)
        let gauge = TokenGauge::new(5000, 4000);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
        gauge.render(Rect::new(0, 0, 40, 1), &mut buf);
        // Should not panic
    }

    #[test]
    fn gauge_zero_budget() {
        let gauge = TokenGauge::new(0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
        gauge.render(Rect::new(0, 0, 40, 1), &mut buf);
    }

    #[test]
    fn gauge_with_block() {
        let gauge = TokenGauge::new(1000, 4000)
            .block(Block::default().title("Budget"));
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        gauge.render(Rect::new(0, 0, 40, 3), &mut buf);
    }
}
