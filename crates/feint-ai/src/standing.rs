use crate::runner::RunReport;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standing {
    pub matches: u32,
    pub won: u32,
    pub mine: u32,
    pub theirs: u32,
    pub fakes: u32,
}

impl Standing {
    pub fn add(&mut self, report: &RunReport, me: usize) {
        let points = |seat: usize| {
            report
                .score
                .get(seat)
                .map_or(0, |&p| u32::try_from(p).unwrap_or(0))
        };
        let (mine, theirs) = (points(me), points(1 - me));
        self.matches += 1;
        self.won += u32::from(report.finished && mine > theirs);
        self.mine += mine;
        self.theirs += theirs;
        let mut faking = false;
        for action in report
            .actions
            .iter()
            .filter(|a| usize::from(a.player) == me)
        {
            if action.deceptive && !faking {
                self.fakes += 1;
            }
            faking = action.deceptive;
        }
    }

    #[must_use]
    pub fn share(&self) -> f64 {
        let total = self.mine + self.theirs;
        if total == 0 {
            0.0
        } else {
            f64::from(self.mine) / f64::from(total)
        }
    }

    #[must_use]
    pub fn fakes_per_match(&self) -> f64 {
        if self.matches == 0 {
            0.0
        } else {
            f64::from(self.fakes) / f64::from(self.matches)
        }
    }
}
