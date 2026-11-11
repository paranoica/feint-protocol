use airhockey::OBSERVATION_LEN;

use crate::Policy;
use crate::pilot::EXTRA_FEATURES;
use crate::safetensors::{SafeTensors, SafeTensorsError};

pub const FORMAT: &str = "feint-mlp-v1";
pub const LAYOUT: &str = "airhockey-v3:anticipated-observation16+reaction_s+physique";
const FEATURES: usize = OBSERVATION_LEN + EXTRA_FEATURES;
const OUTPUTS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlpError {
    Format(SafeTensorsError),
    Metadata(&'static str),
    Shape(&'static str),
    Scales,
}

#[derive(Clone, Debug)]
struct Dense {
    inputs: usize,
    weight: Vec<f32>,
    bias: Vec<f32>,
}

impl Dense {
    fn forward(&self, x: &[f32], out: &mut Vec<f32>, relu: bool) {
        out.clear();
        for (row, &bias) in self.weight.chunks_exact(self.inputs).zip(&self.bias) {
            let mut acc = bias;
            for (&w, &v) in row.iter().zip(x) {
                acc += w * v;
            }
            out.push(if !relu || acc > 0.0 { acc } else { 0.0 });
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mlp {
    inverse: Vec<f32>,
    layers: [Dense; 3],
    a: Vec<f32>,
    b: Vec<f32>,
}

type Raw = (Vec<usize>, Vec<f32>, Vec<f32>);

fn layer(st: &SafeTensors<'_>, i: usize) -> Result<Raw, MlpError> {
    let names = [
        ("layer0.weight", "layer0.bias"),
        ("layer1.weight", "layer1.bias"),
        ("layer2.weight", "layer2.bias"),
    ];
    let (w, b) = names[i];
    let weight = st.tensor(w).ok_or(MlpError::Shape("missing layer"))?;
    let bias = st.tensor(b).ok_or(MlpError::Shape("missing layer"))?;
    let shape = weight.shape().to_vec();
    if shape.len() != 2 || bias.shape() != [shape[0]] {
        return Err(MlpError::Shape(
            "a layer is not rows x columns with a bias per row",
        ));
    }
    Ok((shape, weight.values(), bias.values()))
}

fn require(
    st: &SafeTensors<'_>,
    key: &str,
    want: &str,
    what: &'static str,
) -> Result<(), MlpError> {
    if st.metadata(key) == Some(want) {
        Ok(())
    } else {
        Err(MlpError::Metadata(what))
    }
}

fn inverse_scales(text: &str) -> Result<Vec<f32>, MlpError> {
    let values = text
        .split(',')
        .map(|part| match part.parse::<f32>() {
            Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
            _ => Err(MlpError::Scales),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() == FEATURES {
        Ok(values)
    } else {
        Err(MlpError::Scales)
    }
}

impl Mlp {
    pub fn from_safetensors(bytes: &[u8]) -> Result<Self, MlpError> {
        let st = SafeTensors::parse(bytes).map_err(MlpError::Format)?;
        require(&st, "format", FORMAT, "format")?;
        require(&st, "layout", LAYOUT, "layout")?;
        require(&st, "activation", "relu", "activation")?;
        require(&st, "features", &FEATURES.to_string(), "features")?;
        require(&st, "outputs", &OUTPUTS.to_string(), "outputs")?;
        let (s0, w0, b0) = layer(&st, 0)?;
        let (s1, w1, b1) = layer(&st, 1)?;
        let (s2, w2, b2) = layer(&st, 2)?;
        if s0[1] != FEATURES || s1[1] != s0[0] || s2[1] != s1[0] || s2[0] != OUTPUTS {
            return Err(MlpError::Shape(
                "layers do not chain from the features to the outputs",
            ));
        }
        require(&st, "hidden", &format!("{},{}", s0[0], s1[0]), "hidden")?;
        let inverse = inverse_scales(st.metadata("inv_scales").ok_or(MlpError::Scales)?)?;
        let dense = |inputs: usize, weight: Vec<f32>, bias: Vec<f32>| Dense {
            inputs,
            weight,
            bias,
        };
        Ok(Self {
            inverse,
            layers: [
                dense(s0[1], w0, b0),
                dense(s1[1], w1, b1),
                dense(s2[1], w2, b2),
            ],
            a: Vec::new(),
            b: Vec::new(),
        })
    }

    pub fn inverse_scales(&self) -> &[f32] {
        &self.inverse
    }
}

impl Policy for Mlp {
    fn act(&mut self, features: &[f32], out: &mut [f32]) {
        self.a.clear();
        self.a
            .extend(features.iter().zip(&self.inverse).map(|(&f, &s)| f * s));
        let [first, second, last] = &self.layers;
        first.forward(&self.a, &mut self.b, true);
        second.forward(&self.b, &mut self.a, true);
        last.forward(&self.a, &mut self.b, false);
        for (slot, &v) in out.iter_mut().zip(&self.b) {
            *slot = v;
        }
    }
}
