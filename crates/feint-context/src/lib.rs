use feint_safetensors::{SafeTensors, SafeTensorsError};
use feint_turn::exp;

pub const FORMAT: &str = "feint-context-v1";
pub const FEATURES: usize = 20;
pub const KICKS: usize = 10;
pub const OUTPUTS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum ContextError {
    File(SafeTensorsError),
    Format(String),
    Layout(String),
    Missing(String),
    Shape(String),
}

impl From<SafeTensorsError> for ContextError {
    fn from(e: SafeTensorsError) -> Self {
        Self::File(e)
    }
}

#[derive(Clone, Debug)]
struct Linear {
    weight: Vec<f32>,
    bias: Vec<f32>,
    inputs: usize,
}

impl Linear {
    fn apply(&self, x: &[f32]) -> Vec<f32> {
        self.bias
            .iter()
            .enumerate()
            .map(|(j, &b)| {
                let row = &self.weight[j * self.inputs..(j + 1) * self.inputs];
                let mut acc = b;
                for (w, v) in row.iter().zip(x) {
                    acc += w * v;
                }
                acc
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
struct Norm {
    gain: Vec<f32>,
    bias: Vec<f32>,
    eps: f32,
}

impl Norm {
    fn apply(&self, x: &[f32]) -> Vec<f32> {
        let n = x.len() as f32;
        let mut sum = 0.0f32;
        for v in x {
            sum += v;
        }
        let mean = sum / n;
        let mut var = 0.0f32;
        for v in x {
            var += (v - mean) * (v - mean);
        }
        let scale = 1.0 / (var / n + self.eps).sqrt();
        x.iter()
            .zip(self.gain.iter().zip(&self.bias))
            .map(|(v, (g, b))| (v - mean) * scale * g + b)
            .collect()
    }
}

#[derive(Clone, Debug)]
struct Block {
    ln1: Norm,
    qkv: Linear,
    out: Linear,
    ln2: Norm,
    ff1: Linear,
    ff2: Linear,
}

#[derive(Clone, Debug)]
pub struct ContextModel {
    width: usize,
    heads: usize,
    start: Vec<f32>,
    embed: Linear,
    pos: Vec<f32>,
    blocks: Vec<Block>,
    ln_f: Norm,
    head: Linear,
}

#[must_use]
pub fn softmax(logits: &[f64]) -> Vec<f64> {
    let top = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let e: Vec<f64> = logits.iter().map(|&v| exp(v - top)).collect();
    let total: f64 = e.iter().sum();
    e.into_iter().map(|v| v / total).collect()
}

#[must_use]
pub fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + exp(-x))
    } else {
        let e = exp(x);
        e / (1.0 + e)
    }
}

fn number(st: &SafeTensors<'_>, key: &str) -> Result<usize, ContextError> {
    st.metadata(key)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ContextError::Format(format!("metadata {key}")))
}

fn values(st: &SafeTensors<'_>, name: &str, shape: &[usize]) -> Result<Vec<f32>, ContextError> {
    let t = st
        .tensor(name)
        .ok_or_else(|| ContextError::Missing(name.into()))?;
    if t.shape() != shape {
        return Err(ContextError::Shape(format!(
            "{name}: {:?} not {shape:?}",
            t.shape()
        )));
    }
    Ok(t.values())
}

fn linear(
    st: &SafeTensors<'_>,
    name: &str,
    outputs: usize,
    inputs: usize,
) -> Result<Linear, ContextError> {
    Ok(Linear {
        weight: values(st, &format!("{name}.weight"), &[outputs, inputs])?,
        bias: values(st, &format!("{name}.bias"), &[outputs])?,
        inputs,
    })
}

fn norm(st: &SafeTensors<'_>, name: &str, width: usize, eps: f32) -> Result<Norm, ContextError> {
    Ok(Norm {
        gain: values(st, &format!("{name}.weight"), &[width])?,
        bias: values(st, &format!("{name}.bias"), &[width])?,
        eps,
    })
}

impl ContextModel {
    pub fn from_safetensors(bytes: &[u8], layout: &str) -> Result<Self, ContextError> {
        let st = SafeTensors::parse(bytes)?;
        if st.metadata("format") != Some(FORMAT) {
            return Err(ContextError::Format(format!("{:?}", st.metadata("format"))));
        }
        if st.metadata("layout") != Some(layout) {
            return Err(ContextError::Layout(format!("{:?}", st.metadata("layout"))));
        }
        if st.metadata("activation") != Some("relu") {
            return Err(ContextError::Format("activation".into()));
        }
        let width = number(&st, "width")?;
        let layers = number(&st, "layers")?;
        let heads = number(&st, "heads")?;
        if heads == 0 || width == 0 || width % heads != 0 {
            return Err(ContextError::Format("width and heads".into()));
        }
        let eps: f32 = st
            .metadata("ln_eps")
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| ContextError::Format("ln_eps".into()))?;
        let blocks = (0..layers)
            .map(|i| {
                let b = format!("blocks.{i}");
                Ok(Block {
                    ln1: norm(&st, &format!("{b}.ln1"), width, eps)?,
                    qkv: linear(&st, &format!("{b}.qkv"), 3 * width, width)?,
                    out: linear(&st, &format!("{b}.out"), width, width)?,
                    ln2: norm(&st, &format!("{b}.ln2"), width, eps)?,
                    ff1: linear(&st, &format!("{b}.ff1"), 4 * width, width)?,
                    ff2: linear(&st, &format!("{b}.ff2"), width, 4 * width)?,
                })
            })
            .collect::<Result<Vec<_>, ContextError>>()?;
        Ok(Self {
            width,
            heads,
            start: values(&st, "start", &[FEATURES])?,
            embed: linear(&st, "embed", width, FEATURES)?,
            pos: values(&st, "pos", &[KICKS, width])?,
            blocks,
            ln_f: norm(&st, "ln_f", width, eps)?,
            head: linear(&st, "head", OUTPUTS, width)?,
        })
    }

    fn attend(&self, block: &Block, xs: &[Vec<f32>]) -> Vec<Vec<f32>> {
        let w = self.width;
        let d = w / self.heads;
        let qkv: Vec<Vec<f32>> = xs
            .iter()
            .map(|x| block.qkv.apply(&block.ln1.apply(x)))
            .collect();
        let scale = 1.0 / (d as f32).sqrt();
        (0..xs.len())
            .map(|i| {
                let mut mixed = vec![0.0f32; w];
                for h in 0..self.heads {
                    let q = &qkv[i][h * d..(h + 1) * d];
                    let scores: Vec<f64> = (0..=i)
                        .map(|j| {
                            let k = &qkv[j][w + h * d..w + (h + 1) * d];
                            let mut dot = 0.0f32;
                            for (a, b) in q.iter().zip(k) {
                                dot += a * b;
                            }
                            f64::from(dot * scale)
                        })
                        .collect();
                    let p = softmax(&scores);
                    for (j, &pj) in p.iter().enumerate() {
                        let v = &qkv[j][2 * w + h * d..2 * w + (h + 1) * d];
                        for (m, &vv) in mixed[h * d..(h + 1) * d].iter_mut().zip(v) {
                            *m += pj as f32 * vv;
                        }
                    }
                }
                block.out.apply(&mixed)
            })
            .collect()
    }

    #[must_use]
    pub fn predict(&self, tokens: &[[f32; FEATURES]]) -> [f32; OUTPUTS] {
        let kept = &tokens[tokens.len().saturating_sub(KICKS - 1)..];
        let w = self.width;
        let mut xs: Vec<Vec<f32>> = std::iter::once(self.start.as_slice())
            .chain(kept.iter().map(<[f32; FEATURES]>::as_slice))
            .enumerate()
            .map(|(i, t)| {
                let mut x = self.embed.apply(t);
                for (v, p) in x.iter_mut().zip(&self.pos[i * w..(i + 1) * w]) {
                    *v += p;
                }
                x
            })
            .collect();
        for block in &self.blocks {
            let mixed = self.attend(block, &xs);
            for (x, m) in xs.iter_mut().zip(mixed) {
                for (v, add) in x.iter_mut().zip(m) {
                    *v += add;
                }
                let ff = block.ff2.apply(
                    &block
                        .ff1
                        .apply(&block.ln2.apply(x))
                        .into_iter()
                        .map(|v| if v > 0.0 { v } else { 0.0 })
                        .collect::<Vec<_>>(),
                );
                for (v, add) in x.iter_mut().zip(ff) {
                    *v += add;
                }
            }
        }
        let last = xs
            .last()
            .map_or_else(Vec::new, |x| self.head.apply(&self.ln_f.apply(x)));
        let mut out = [0.0; OUTPUTS];
        for (o, v) in out.iter_mut().zip(last) {
            *o = v;
        }
        out
    }
}
