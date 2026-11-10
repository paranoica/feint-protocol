use feint_protocol::{
    ActionT, AdapterError, BodyParamsT, EventT, GameAdapter, ObservationT, PlayerId,
};

use crate::playbook::Controller;

#[derive(Clone, Debug, PartialEq)]
pub struct RunHeader {
    pub game_id: String,
    pub game_version: u32,
    pub seed: u64,
    pub bodies: Vec<BodyParamsT>,
    pub levels: [u8; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunReport {
    pub header: RunHeader,
    pub score: Vec<i64>,
    pub ticks: u64,
    pub finished: bool,
    pub actions: Vec<ActionT>,
    pub digest: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub header: RunHeader,
    pub actions: Vec<ActionT>,
    pub ticks: u64,
    pub score: Vec<i64>,
}

impl RunReport {
    #[must_use]
    pub fn record(&self) -> Record {
        Record {
            header: self.header.clone(),
            actions: self.actions.clone(),
            ticks: self.ticks,
            score: self.score.clone(),
        }
    }
}

pub struct Runner {
    header: RunHeader,
    seen: [ObservationT; 2],
    events: Vec<EventT>,
    actions: Vec<ActionT>,
    ticks: u64,
}

impl Runner {
    pub fn start<A: GameAdapter>(
        adapter: &mut A,
        controllers: [&mut dyn Controller<A>; 2],
        header: RunHeader,
    ) -> Result<Self, AdapterError> {
        adapter.reset(header.seed, &header.bodies)?;
        let spec = adapter.spec();
        for (me, controller) in controllers.into_iter().enumerate() {
            controller.new_match(&spec, me as PlayerId, header.seed);
        }
        Ok(Self {
            header,
            seen: [ObservationT::default(), ObservationT::default()],
            events: Vec::new(),
            actions: Vec::new(),
            ticks: 0,
        })
    }

    pub fn step<A: GameAdapter>(
        &mut self,
        adapter: &mut A,
        controllers: [&mut dyn Controller<A>; 2],
    ) -> bool {
        self.events.clear();
        if adapter.result().is_some() {
            return false;
        }
        let mut controllers = controllers;
        for (me, controller) in controllers.iter_mut().enumerate() {
            adapter.observe(adapter.live(), me as PlayerId, &mut self.seen[me]);
            if let Some(action) = controller.on_tick(adapter, &self.seen[me])
                && adapter.apply(&action).is_ok()
            {
                self.actions.push(action);
            }
        }
        adapter.advance(&mut self.events);
        for event in &self.events {
            for controller in &mut controllers {
                controller.on_event(event);
            }
        }
        self.ticks += 1;
        true
    }

    #[must_use]
    pub fn events(&self) -> &[EventT] {
        &self.events
    }

    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    #[must_use]
    pub fn actions(&self) -> &[ActionT] {
        &self.actions
    }

    pub fn forget_actions(&mut self) {
        self.actions.clear();
    }

    #[must_use]
    pub fn report<A: GameAdapter>(&self, adapter: &A) -> RunReport {
        let result = adapter.result();
        let score = match &result {
            Some(result) => result.score.clone().unwrap_or_default(),
            None => {
                let mut seen = ObservationT::default();
                adapter.observe(adapter.live(), 0, &mut seen);
                seen.score.unwrap_or_default()
            }
        };
        RunReport {
            header: self.header.clone(),
            score,
            ticks: self.ticks,
            finished: result.is_some(),
            actions: self.actions.clone(),
            digest: adapter.digest(adapter.live()),
        }
    }

    pub fn finish<A: GameAdapter>(
        mut self,
        adapter: &mut A,
        controllers: [&mut dyn Controller<A>; 2],
    ) -> RunReport {
        for (me, controller) in controllers.into_iter().enumerate() {
            adapter.observe(adapter.live(), me as PlayerId, &mut self.seen[me]);
            controller.end_match(adapter, &self.seen[me]);
        }
        self.report(adapter)
    }
}

pub fn run_match<A: GameAdapter>(
    adapter: &mut A,
    controllers: [&mut dyn Controller<A>; 2],
    header: RunHeader,
    max_ticks: u64,
) -> Result<RunReport, AdapterError> {
    let [first, second] = controllers;
    let mut runner = Runner::start(adapter, [&mut *first, &mut *second], header)?;
    while runner.ticks() < max_ticks && runner.step(adapter, [&mut *first, &mut *second]) {}
    Ok(runner.finish(adapter, [first, second]))
}

pub fn replay<A: GameAdapter>(adapter: &mut A, report: &RunReport) -> Result<u64, AdapterError> {
    replay_record(adapter, &report.record())
}

pub fn replay_record<A: GameAdapter>(
    adapter: &mut A,
    record: &Record,
) -> Result<u64, AdapterError> {
    adapter.reset(record.header.seed, &record.header.bodies)?;
    let mut pending = record.actions.iter().peekable();
    let mut events = Vec::new();
    for tick in 0..record.ticks {
        if adapter.result().is_some() {
            return Err(AdapterError::MatchOver);
        }
        while let Some(action) = pending.next_if(|a| a.tick <= tick) {
            adapter.apply(action)?;
        }
        events.clear();
        adapter.advance(&mut events);
    }
    if let Some(late) = pending.next() {
        return Err(AdapterError::WrongTick {
            expected: record.ticks,
            got: late.tick,
        });
    }
    Ok(adapter.digest(adapter.live()))
}
