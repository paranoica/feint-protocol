use feint_protocol::{ActionT, AdapterError, BodyParamsT, GameAdapter, ObservationT, PlayerId};

use crate::bot::Controller;

/// Everything a match needs to be replayed besides its actions (FNT-D6, FNT-D9).
#[derive(Clone, Debug, PartialEq)]
pub struct RunHeader {
    pub game_id: String,
    pub game_version: u32,
    pub seed: u64,
    pub bodies: Vec<BodyParamsT>,
    pub levels: [u8; 2],
}

/// A played match: its header, outcome and every action the game accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct RunReport {
    pub header: RunHeader,
    pub score: Vec<i64>,
    pub ticks: u64,
    /// The game reported a result; otherwise the tick cap ended it.
    pub finished: bool,
    pub actions: Vec<ActionT>,
    pub digest: u64,
}

/// Play one match between two controllers, at most `max_ticks` ticks.
///
/// # Errors
/// The adapter refused the header's bodies.
pub fn run_match<A: GameAdapter>(
    adapter: &mut A,
    controllers: [&mut dyn Controller<A>; 2],
    header: RunHeader,
    max_ticks: u64,
) -> Result<RunReport, AdapterError> {
    adapter.reset(header.seed, &header.bodies)?;
    let spec = adapter.spec();
    let mut controllers = controllers;
    for (me, controller) in controllers.iter_mut().enumerate() {
        controller.new_match(&spec, me as PlayerId, header.seed);
    }
    let mut seen = [ObservationT::default(), ObservationT::default()];
    let mut events = Vec::new();
    let mut actions = Vec::new();
    let mut ticks = 0;
    while ticks < max_ticks && adapter.result().is_none() {
        for (me, controller) in controllers.iter_mut().enumerate() {
            adapter.observe(adapter.live(), me as PlayerId, &mut seen[me]);
            if let Some(action) = controller.on_tick(adapter, &seen[me])
                && adapter.apply(&action).is_ok()
            {
                actions.push(action);
            }
        }
        events.clear();
        adapter.advance(&mut events);
        for event in &events {
            for controller in &mut controllers {
                controller.on_event(event);
            }
        }
        ticks += 1;
    }
    let result = adapter.result();
    let score = match &result {
        Some(result) => result.score.clone().unwrap_or_default(),
        None => {
            adapter.observe(adapter.live(), 0, &mut seen[0]);
            seen[0].score.clone().unwrap_or_default()
        }
    };
    Ok(RunReport {
        header,
        score,
        ticks,
        finished: result.is_some(),
        actions,
        digest: adapter.digest(adapter.live()),
    })
}

/// Re-play a report's actions on a fresh adapter; returns the final digest.
///
/// # Errors
/// The adapter refused the header's bodies or a recorded action.
pub fn replay<A: GameAdapter>(adapter: &mut A, report: &RunReport) -> Result<u64, AdapterError> {
    adapter.reset(report.header.seed, &report.header.bodies)?;
    let mut pending = report.actions.iter().peekable();
    let mut events = Vec::new();
    for tick in 0..report.ticks {
        while let Some(action) = pending.next_if(|a| a.tick <= tick) {
            // Recorded actions were accepted once; a refusal now means the replay diverged.
            adapter.apply(action)?;
        }
        events.clear();
        adapter.advance(&mut events);
    }
    Ok(adapter.digest(adapter.live()))
}
