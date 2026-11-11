use std::fs::File;

use airhockey::{AirHockey, Params};
use arrow::array::{Array, AsArray};
use arrow::datatypes::{Float64Type, Int8Type, UInt8Type, UInt64Type};
use feint_ai::{decode_record, replay_record};
use feint_arena::{parquet_out, verify};
use feint_protocol::{EventKind, GameAdapter};
use feint_web::{HubGame, Session};
use penalty::Penalty;

mod common;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("feint-arena-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("match.parquet")
}

#[test]
fn parquet_rows_match_facts_gathered_without_the_writer() {
    let mut session = Session::new(2, 5).unwrap();
    let mut goal_ticks = Vec::new();
    while !session.finished() && session.tick() < 240 * 60 * 15 {
        let p = session.positions();
        session.set_pointer(p[0], p[1]);
        let tick = session.tick();
        let goals = session
            .advance(1)
            .iter()
            .filter(|e| e.kind == EventKind::Score)
            .count();
        if goals > 0 {
            goal_ticks.push(tick);
        }
    }
    assert!(session.finished());
    let bytes = session.record();
    let record = decode_record(&bytes).unwrap();
    let verified = verify(&bytes, None).unwrap();
    let path = scratch("facts");
    parquet_out::write(
        &verified.frames,
        verified.columns,
        2,
        record.header.seed,
        0,
        File::create(&path).unwrap(),
    )
    .unwrap();

    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap())
        .unwrap()
        .build()
        .unwrap();
    let batches: Vec<_> = reader.map(Result::unwrap).collect();
    let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(rows as u64, record.ticks, "one row per tick");
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches).unwrap();
    let col = |name: &str| batch.column_by_name(name).unwrap().clone();
    let ticks = col("tick");
    let ticks = ticks.as_primitive::<UInt64Type>();
    assert_eq!(ticks.value(0), 0);
    assert_eq!(ticks.value(rows - 1), record.ticks - 1);

    let mut game = AirHockey::new(Params::default(), 2);
    replay_record(&mut game, &record).unwrap();
    let world = &game.live().world;
    let last = |name: &str| col(name).as_primitive::<Float64Type>().value(rows - 1);
    assert_eq!(last("puck_x"), world.puck.pos.x);
    assert_eq!(last("puck_vy"), world.puck.vel.y);
    assert_eq!(last("m0_y"), world.mallets[0].pos.y);
    assert_eq!(last("m1_vx"), world.mallets[1].vel.x);

    let events = col("events");
    let events = events.as_list::<i32>();
    let mut parquet_goals = Vec::new();
    for row in 0..rows {
        let items = events.value(row);
        let items = items.as_struct();
        let kinds = items.column(0).as_primitive::<Int8Type>();
        if (0..kinds.len()).any(|i| kinds.value(i) == EventKind::Score.0) {
            parquet_goals.push(ticks.value(row));
        }
    }
    assert_eq!(
        parquet_goals, goal_ticks,
        "score events sit on the ticks the session reported"
    );

    let intents = col("intent");
    let intents = intents.as_primitive::<UInt8Type>();
    let mut last_bot = None;
    let mut actions = record.actions.iter().filter(|a| a.player == 1).peekable();
    for row in 0..rows {
        while let Some(a) = actions.next_if(|a| a.tick <= ticks.value(row)) {
            last_bot = Some(a.intent);
        }
        assert_eq!(
            intents.is_valid(row).then(|| intents.value(row)),
            last_bot,
            "row {row}"
        );
    }
    let level = col("level");
    assert!(
        level
            .as_primitive::<UInt8Type>()
            .iter()
            .all(|v| v == Some(2))
    );
    let seed = col("seed");
    assert!(
        seed.as_primitive::<UInt64Type>()
            .iter()
            .all(|v| v == Some(record.header.seed))
    );
    let checkpoint = col("checkpoint");
    assert!(
        checkpoint
            .as_primitive::<UInt64Type>()
            .iter()
            .all(|v| v == Some(0))
    );
}

#[test]
fn the_checkpoint_column_is_written() {
    let id = feint_ml::registry::CHECKPOINTS[0].id;
    let mut session = Session::trained(5, 6, id).unwrap();
    for _ in 0..200 {
        let p = session.positions();
        session.set_pointer(p[0], p[1]);
        session.advance(4);
    }
    let verified = verify(&session.record(), None).unwrap();
    let path = scratch("checkpoint");
    parquet_out::write(
        &verified.frames,
        verified.columns,
        5,
        6,
        id,
        File::create(&path).unwrap(),
    )
    .unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap())
        .unwrap()
        .build()
        .unwrap();
    let batches: Vec<_> = reader.map(Result::unwrap).collect();
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches).unwrap();
    let column = batch.column_by_name("checkpoint").unwrap();
    assert_eq!(column.len(), 800);
    assert!(
        column
            .as_primitive::<UInt64Type>()
            .iter()
            .all(|v| v == Some(id))
    );
}

#[test]
fn a_tick_with_several_events_keeps_them_all() {
    let frame = |tick: u64, events: Vec<(i8, i8)>| feint_arena::Frame {
        tick,
        values: vec![Some(0.0); 12],
        events,
        intent: None,
    };
    let frames = vec![
        frame(0, vec![(0, 1), (1, 0)]),
        frame(1, vec![]),
        frame(2, vec![(2, -1)]),
    ];
    let path = scratch("several");
    parquet_out::write(
        &frames,
        AirHockey::COLUMNS,
        1,
        9,
        0,
        File::create(&path).unwrap(),
    )
    .unwrap();
    let batch = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let events = batch
        .column_by_name("events")
        .unwrap()
        .as_list::<i32>()
        .clone();
    let lens: Vec<usize> = (0..3).map(|r| events.value(r).len()).collect();
    assert_eq!(lens, vec![2, 0, 1]);
    let first = events.value(0);
    let first = first.as_struct();
    let kinds: Vec<i8> = first.column(0).as_primitive::<Int8Type>().values().to_vec();
    let players: Vec<i8> = first.column(1).as_primitive::<Int8Type>().values().to_vec();
    assert_eq!((kinds, players), (vec![0, 1], vec![1, 0]));
    assert!(batch.column_by_name("intent").unwrap().is_null(0));
}

#[test]
fn a_penalty_match_is_written_with_its_own_columns() {
    let (game, kicks) = common::shootout(2, 4, None);
    let verified = verify(&game.record(), None).unwrap();
    let path = scratch("penalty");
    parquet_out::write(
        &verified.frames,
        verified.columns,
        2,
        4,
        0,
        File::create(&path).unwrap(),
    )
    .unwrap();
    let batches: Vec<_> = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let batch = arrow::compute::concat_batches(&batches[0].schema(), &batches).unwrap();
    assert_eq!(batch.num_rows() as u64, verified.record.ticks);
    assert!(batch.column_by_name("puck_x").is_none());
    let last = |name: &str| {
        let c = batch.column_by_name(name).unwrap();
        c.as_primitive::<Float64Type>().value(c.len() - 1)
    };
    let live = game.score();
    assert_eq!(
        [last("score0"), last("score1")],
        [f64::from(live[0]), f64::from(live[1])]
    );
    let first = batch.column_by_name("kicks").unwrap();
    assert_eq!(
        first.as_primitive::<Float64Type>().value(0),
        0.0,
        "the first row is before any kick"
    );
    assert_eq!(last("kicks"), f64::from(kicks), "one kick per goal or save");
    for name in ["aim", "dive_target"] {
        let c = batch.column_by_name(name).unwrap();
        let c = c.as_primitive::<Float64Type>();
        assert!(c.is_null(0), "{name} is empty before the first kick");
        assert!(c.null_count() > 0 && c.null_count() < c.len(), "{name}");
        assert!(c.iter().flatten().all(f64::is_finite), "{name} has no NaN");
    }
    let shooter = batch.column_by_name("shooter").unwrap();
    assert_eq!(shooter.null_count(), 0);
    assert!(
        !batch
            .schema()
            .field_with_name("shooter")
            .unwrap()
            .is_nullable()
    );

    let short = vec![feint_arena::Frame {
        tick: 0,
        values: vec![Some(0.0); 3],
        events: vec![],
        intent: None,
    }];
    assert!(
        parquet_out::write(
            &short,
            Penalty::COLUMNS,
            1,
            1,
            0,
            File::create(&path).unwrap()
        )
        .is_err(),
        "a frame narrower than its columns"
    );
    let mut gap = verified.frames[..1].to_vec();
    let at = Penalty::COLUMNS
        .iter()
        .position(|c| c.name == "shooter")
        .unwrap();
    gap[0].values[at] = None;
    assert!(
        parquet_out::write(
            &gap,
            Penalty::COLUMNS,
            1,
            1,
            0,
            File::create(&path).unwrap()
        )
        .is_err(),
        "an empty value in a column that is never empty"
    );
}
