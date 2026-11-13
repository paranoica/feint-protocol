use believe::Believe;
use feint_turn::{TurnBatch, TurnError, TurnOutputs};
use kuhn::Kuhn;
use numpy::{PyArray1, PyArrayMethods, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

enum Any {
    Kuhn(TurnBatch<Kuhn>),
    Believe(TurnBatch<Believe>),
}

macro_rules! each {
    ($any:expr, $b:ident => $body:expr) => {
        match $any {
            Any::Kuhn($b) => $body,
            Any::Believe($b) => $body,
        }
    };
}

fn refused(error: TurnError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

#[pyclass(name = "TurnBatch", module = "feint_gym", unsendable)]
pub struct PyTurnBatch {
    batch: Any,
    game: String,
}

impl PyTurnBatch {
    fn arrays<'py>(&self, py: Python<'py>, out: &TurnOutputs) -> PyResult<Bound<'py, PyDict>> {
        let envs = each!(&self.batch, b => b.envs());
        let features = each!(&self.batch, b => b.features());
        let actions = each!(&self.batch, b => b.actions());
        let dict = PyDict::new(py);
        dict.set_item(
            "obs",
            PyArray1::from_slice(py, &out.obs).reshape([envs, 2, features])?,
        )?;
        dict.set_item(
            "legal",
            PyArray1::from_slice(py, &out.legal).reshape([envs, 2, actions])?,
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
        dict.set_item(
            "score",
            PyArray1::from_slice(py, &out.score).reshape([envs, 2])?,
        )?;
        Ok(dict)
    }
}

#[pymethods]
impl PyTurnBatch {
    #[new]
    fn new(game: &str, seats: Vec<(String, String)>) -> PyResult<Self> {
        let pairs: Vec<[String; 2]> = seats.into_iter().map(|(a, b)| [a, b]).collect();
        let batch = match game {
            "kuhn" => Any::Kuhn(TurnBatch::new(&pairs).map_err(refused)?),
            "believe" => Any::Believe(TurnBatch::new(&pairs).map_err(refused)?),
            other => return Err(PyValueError::new_err(format!("unknown game {other:?}"))),
        };
        Ok(Self {
            batch,
            game: game.to_string(),
        })
    }

    fn __len__(&self) -> usize {
        each!(&self.batch, b => b.envs())
    }

    #[getter]
    fn game(&self) -> String {
        self.game.clone()
    }

    #[getter]
    fn features(&self) -> usize {
        each!(&self.batch, b => b.features())
    }

    #[getter]
    fn actions(&self) -> usize {
        each!(&self.batch, b => b.actions())
    }

    #[getter]
    fn scales(&self) -> Vec<f32> {
        each!(&self.batch, b => b.scales())
    }

    fn seats(&self, env: usize) -> PyResult<(String, String)> {
        let names = each!(&self.batch, b => b.seat_names().get(env).cloned());
        names
            .map(|[a, b]| (a, b))
            .ok_or_else(|| PyValueError::new_err(format!("no env {env}")))
    }

    fn reset<'py>(&mut self, py: Python<'py>, seeds: Vec<u64>) -> PyResult<Bound<'py, PyDict>> {
        let out = each!(&mut self.batch, b => b.reset(&seeds)).map_err(refused)?;
        self.arrays(py, &out)
    }

    fn step<'py>(
        &mut self,
        py: Python<'py>,
        actions: PyReadonlyArrayDyn<'py, i32>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let envs = self.__len__();
        let shape = actions.shape().to_vec();
        if shape != [envs, 2] {
            return Err(PyValueError::new_err(format!(
                "actions of shape {shape:?}, expected [{envs}, 2]"
            )));
        }
        let actions = actions.as_array().iter().copied().collect::<Vec<i32>>();
        let out = each!(&mut self.batch, b => b.step(&actions)).map_err(refused)?;
        self.arrays(py, &out)
    }
}
