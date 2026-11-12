use feint_ml::Params;
use feint_ml::batch::{FEATURES, OUTPUTS, feature_scales};
use feint_ml::imitation::{
    ImitationError, encode_record, imitation as extract, pilot_match as play,
};
use feint_ml::mlp::Mlp;
use feint_ml::{BatchError, BatchSpec, HockeyBatch, Outputs, Seat};
use numpy::{PyArray1, PyArrayMethods, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use std::sync::{Mutex, MutexGuard, TryLockError};

fn refused(error: BatchError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn seat(name: &str) -> PyResult<Seat> {
    match name {
        "learner" => Ok(Seat::Learner),
        "human" => Ok(Seat::Human),
        "reader" => Ok(Seat::Reader { anticipates: false }),
        "reader:anticipating" => Ok(Seat::Reader { anticipates: true }),
        other => {
            let level = other
                .strip_prefix("level:")
                .and_then(|level| level.parse().ok())
                .map(Seat::Level);
            let frozen = other
                .strip_prefix("frozen:")
                .and_then(|index| index.parse().ok())
                .map(Seat::Frozen);
            level
                .or(frozen)
                .ok_or_else(|| PyValueError::new_err(format!("unknown seat {other:?}")))
        }
    }
}

type EpisodeView = (u64, (u32, u32), (f64, f64));

fn name(seat: Seat) -> String {
    match seat {
        Seat::Learner => "learner".into(),
        Seat::Human => "human".into(),
        Seat::Reader { anticipates: false } => "reader".into(),
        Seat::Reader { anticipates: true } => "reader:anticipating".into(),
        Seat::Level(level) => format!("level:{level}"),
        Seat::Frozen(index) => format!("frozen:{index}"),
    }
}

#[pyclass(name = "HockeyBatch", module = "feint_gym")]
struct PyHockeyBatch {
    batch: Mutex<HockeyBatch>,
    envs: usize,
}

impl PyHockeyBatch {
    fn batch(&self) -> PyResult<MutexGuard<'_, HockeyBatch>> {
        self.batch.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => {
                PyRuntimeError::new_err("the batch is stepping in another thread")
            }
            TryLockError::Poisoned(_) => PyValueError::new_err("the batch panicked earlier"),
        })
    }

    fn run<'py>(
        &self,
        py: Python<'py>,
        work: impl FnOnce(&mut HockeyBatch) -> Result<Outputs, BatchError> + Send,
    ) -> PyResult<Bound<'py, PyDict>> {
        let out = py.detach(|| {
            let mut batch = self
                .batch
                .lock()
                .map_err(|_| String::from("the batch panicked earlier"))?;
            work(&mut batch).map_err(|error| error.to_string())
        });
        arrays(py, &out.map_err(PyValueError::new_err)?, self.envs)
    }
}

fn arrays<'py>(py: Python<'py>, out: &Outputs, envs: usize) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let features = [envs, 2, FEATURES];
    dict.set_item("obs", PyArray1::from_slice(py, &out.obs).reshape(features)?)?;
    dict.set_item(
        "final_obs",
        PyArray1::from_slice(py, &out.final_obs).reshape(features)?,
    )?;
    dict.set_item(
        "ready",
        PyArray1::from_slice(py, &out.ready).reshape([envs, 2])?,
    )?;
    dict.set_item(
        "reward",
        PyArray1::from_slice(py, &out.reward).reshape([envs, 2])?,
    )?;
    dict.set_item("terminated", PyArray1::from_slice(py, &out.terminated))?;
    dict.set_item("truncated", PyArray1::from_slice(py, &out.truncated))?;
    dict.set_item(
        "score",
        PyArray1::from_slice(py, &out.score).reshape([envs, 2])?,
    )?;
    Ok(dict)
}

#[pymethods]
impl PyHockeyBatch {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (seats, seed, cap_ticks = 240 * 600, threads = 1, reaction_ms = (150, 400), physique = (0.4, 1.0), goals_to_win = None, frozen = Vec::new()))]
    fn new(
        seats: Vec<(String, String)>,
        seed: u64,
        cap_ticks: u64,
        threads: usize,
        reaction_ms: (u32, u32),
        physique: (f64, f64),
        goals_to_win: Option<u32>,
        frozen: Vec<Vec<u8>>,
    ) -> PyResult<Self> {
        let seats = seats
            .iter()
            .map(|(a, b)| Ok([seat(a)?, seat(b)?]))
            .collect::<PyResult<Vec<_>>>()?;
        let mut spec = BatchSpec::new(seats, seed);
        spec.cap_ticks = cap_ticks;
        spec.threads = threads;
        spec.reaction_ms = [reaction_ms.0, reaction_ms.1];
        spec.physique = [physique.0, physique.1];
        spec.frozen = frozen;
        if let Some(goals) = goals_to_win {
            spec.params.goals_to_win = goals;
        }
        let batch = HockeyBatch::new(spec).map_err(refused)?;
        Ok(Self {
            envs: batch.len(),
            batch: Mutex::new(batch),
        })
    }

    fn __len__(&self) -> usize {
        self.envs
    }

    #[classattr]
    fn features() -> usize {
        FEATURES
    }

    #[classattr]
    fn outputs() -> usize {
        OUTPUTS
    }

    #[classattr]
    fn scales() -> Vec<f32> {
        feature_scales(&Params::default()).to_vec()
    }

    fn reset<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.run(py, |batch| batch.reset().cloned())
    }

    fn step<'py>(
        &self,
        py: Python<'py>,
        actions: PyReadonlyArrayDyn<'py, f32>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let envs = self.envs;
        let shape = actions.shape().to_vec();
        if shape != [envs, 2, OUTPUTS] {
            return Err(PyValueError::new_err(format!(
                "actions of shape {shape:?}, expected [{envs}, 2, {OUTPUTS}]"
            )));
        }
        if !actions.is_c_contiguous() || !actions.data().is_aligned() {
            return Err(PyValueError::new_err(
                "actions must be a C-contiguous, aligned float32 array: pass np.ascontiguousarray(actions, dtype=np.float32)",
            ));
        }
        let actions = actions.as_slice()?.to_vec();
        self.run(py, move |batch| batch.step(&actions).cloned())
    }

    fn episode(&self, env: usize) -> PyResult<EpisodeView> {
        let e = self
            .batch()?
            .episode(env)
            .ok_or_else(|| PyValueError::new_err(format!("no env {env}")))?;
        Ok((
            e.seed,
            (e.reaction_ms[0], e.reaction_ms[1]),
            (e.physique[0], e.physique[1]),
        ))
    }

    fn seats(&self, env: usize) -> PyResult<(String, String)> {
        let [a, b] = self
            .batch()?
            .seats(env)
            .ok_or_else(|| PyValueError::new_err(format!("no env {env}")))?;
        Ok((name(a), name(b)))
    }

    fn ticks(&self, env: usize) -> PyResult<u64> {
        self.batch()?
            .ticks(env)
            .ok_or_else(|| PyValueError::new_err(format!("no env {env}")))
    }

    fn digest(&self, env: usize) -> PyResult<u64> {
        self.batch()?
            .digest(env)
            .ok_or_else(|| PyValueError::new_err(format!("no env {env}")))
    }
}

fn unusable(error: ImitationError) -> PyErr {
    PyValueError::new_err(format!("{error:?}"))
}

#[pyfunction]
#[pyo3(signature = (record, reaction_ms = 300))]
fn imitation<'py>(
    py: Python<'py>,
    record: &[u8],
    reaction_ms: u32,
) -> PyResult<Bound<'py, PyDict>> {
    let got = py
        .detach(|| extract(record, reaction_ms))
        .map_err(unusable)?;
    let dict = PyDict::new(py);
    dict.set_item(
        "features",
        PyArray1::from_slice(py, &got.features).reshape([got.decisions, FEATURES])?,
    )?;
    dict.set_item(
        "targets",
        PyArray1::from_slice(py, &got.targets).reshape([got.decisions, OUTPUTS])?,
    )?;
    dict.set_item("ticks", got.ticks)?;
    Ok(dict)
}

#[pyfunction]
#[pyo3(signature = (weights, level, seed, reaction_ms = 150, max_ticks = 240 * 600))]
fn pilot_match<'py>(
    py: Python<'py>,
    weights: &[u8],
    level: u8,
    seed: u64,
    reaction_ms: u32,
    max_ticks: u64,
) -> PyResult<Bound<'py, PyDict>> {
    let mlp = Mlp::from_safetensors(weights)
        .map_err(|error| PyValueError::new_err(format!("bad weights: {error:?}")))?;
    let report = py
        .detach(|| play(mlp, level, seed, reaction_ms, max_ticks))
        .map_err(unusable)?;
    let dict = PyDict::new(py);
    dict.set_item("record", PyBytes::new(py, &encode_record(&report.record())))?;
    dict.set_item("score", (report.score[0], report.score[1]))?;
    dict.set_item("ticks", report.ticks)?;
    dict.set_item("finished", report.finished)?;
    Ok(dict)
}

#[pymodule]
fn feint_gym(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyHockeyBatch>()?;
    module.add_function(wrap_pyfunction!(imitation, module)?)?;
    module.add_function(wrap_pyfunction!(pilot_match, module)?)
}
