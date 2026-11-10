use feint_ml::batch::{FEATURES, OUTPUTS};
use feint_ml::{BatchError, BatchSpec, HockeyBatch, Outputs, Seat};
use numpy::{PyArray1, PyArrayMethods, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
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
        other => other
            .strip_prefix("level:")
            .and_then(|level| level.parse().ok())
            .map(Seat::Level)
            .ok_or_else(|| PyValueError::new_err(format!("unknown seat {other:?}"))),
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
    #[pyo3(signature = (seats, seed, cap_ticks = 240 * 600, threads = 1, reaction_ms = (150, 400), physique = (0.4, 1.0), goals_to_win = None))]
    fn new(
        seats: Vec<(String, String)>,
        seed: u64,
        cap_ticks: u64,
        threads: usize,
        reaction_ms: (u32, u32),
        physique: (f64, f64),
        goals_to_win: Option<u32>,
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

#[pymodule]
fn feint_gym(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyHockeyBatch>()
}
