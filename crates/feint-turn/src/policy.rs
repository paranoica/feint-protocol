use feint_safetensors::{SafeTensors, SafeTensorsError};

pub const FORMAT: &str = "feint-mlp-v1";

const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
const INV_LN2: f64 = f64::from_bits(0x3ff7_1547_652b_82fe);

#[must_use]
pub fn turn_layout(game: &str, features: usize, actions: usize) -> String {
    format!("turn-v1:{game}:{features}:{actions}")
}

fn power_of_two(n: i64) -> f64 {
    f64::from_bits(((n + 1023) as u64) << 52)
}

#[must_use]
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.78 {
        return f64::INFINITY;
    }
    if x < -745.2 {
        return 0.0;
    }
    let k = (x * INV_LN2).round();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let mut series = 1.0;
    for n in (1..=13).rev() {
        series = 1.0 + series * r / f64::from(n);
    }
    let k = k as i64;
    if k < -1022 {
        series * power_of_two(k + 600) * power_of_two(-600)
    } else if k > 1023 {
        series * power_of_two(k - 600) * power_of_two(600)
    } else {
        series * power_of_two(k)
    }
}

pub fn masked_softmax(logits: &[f32], legal: &[u8], out: &mut Vec<f32>) {
    out.clear();
    let allowed = |i: usize| legal.get(i) == Some(&1);
    if !(0..logits.len()).any(allowed) {
        out.resize(logits.len(), 0.0);
        return;
    }
    let top = logits
        .iter()
        .enumerate()
        .filter(|&(i, _)| allowed(i))
        .map(|(_, &v)| f64::from(v))
        .fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = logits
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            if allowed(i) {
                exp(f64::from(v) - top)
            } else {
                0.0
            }
        })
        .collect();
    let total: f64 = weights.iter().sum();
    if top.is_finite() && total.is_finite() && total > 0.0 {
        out.extend(weights.iter().map(|w| (w / total) as f32));
        return;
    }
    let count = (0..logits.len()).filter(|&i| allowed(i)).count() as f32;
    out.extend((0..logits.len()).map(|i| if allowed(i) { 1.0 / count } else { 0.0 }));
}

#[must_use]
pub fn sample_index(probabilities: &[f32], legal: &[u8], u: f32) -> Option<usize> {
    let mut sum = 0.0f32;
    let mut last = None;
    for (i, &p) in probabilities.iter().enumerate() {
        if legal.get(i) != Some(&1) {
            continue;
        }
        sum += p;
        last = Some(i);
        if u < sum {
            return last;
        }
    }
    last
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    Format(SafeTensorsError),
    Metadata(&'static str),
    Shape(&'static str),
    Scales,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(e) => write!(f, "not a policy file: {e:?}"),
            Self::Metadata(what) => write!(f, "the file's {what} does not match"),
            Self::Shape(why) => write!(f, "{why}"),
            Self::Scales => write!(
                f,
                "the inverse scales are not one positive number per feature"
            ),
        }
    }
}

impl std::error::Error for PolicyError {}

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
pub struct TurnPolicy {
    inverse: Vec<f32>,
    layers: [Dense; 3],
    a: Vec<f32>,
    b: Vec<f32>,
}

fn require(
    st: &SafeTensors<'_>,
    key: &str,
    want: &str,
    what: &'static str,
) -> Result<(), PolicyError> {
    if st.metadata(key) == Some(want) {
        Ok(())
    } else {
        Err(PolicyError::Metadata(what))
    }
}

fn layer(st: &SafeTensors<'_>, i: usize) -> Result<Dense, PolicyError> {
    let names = [
        ("layer0.weight", "layer0.bias"),
        ("layer1.weight", "layer1.bias"),
        ("layer2.weight", "layer2.bias"),
    ];
    let (w, b) = names[i];
    let weight = st.tensor(w).ok_or(PolicyError::Shape("missing layer"))?;
    let bias = st.tensor(b).ok_or(PolicyError::Shape("missing layer"))?;
    let shape = weight.shape().to_vec();
    if shape.len() != 2 || shape[0] == 0 || bias.shape() != [shape[0]] {
        return Err(PolicyError::Shape(
            "a layer is not rows x columns with a bias per row",
        ));
    }
    let (weight, bias) = (weight.values(), bias.values());
    if weight.iter().chain(&bias).any(|v| !v.is_finite()) {
        return Err(PolicyError::Shape("a weight is not a finite number"));
    }
    Ok(Dense {
        inputs: shape[1],
        weight,
        bias,
    })
}

impl TurnPolicy {
    pub fn from_safetensors(bytes: &[u8], layout: &str) -> Result<Self, PolicyError> {
        let st = SafeTensors::parse(bytes).map_err(PolicyError::Format)?;
        require(&st, "format", FORMAT, "format")?;
        require(&st, "layout", layout, "layout")?;
        require(&st, "activation", "relu", "activation")?;
        let layers = [layer(&st, 0)?, layer(&st, 1)?, layer(&st, 2)?];
        let rows = |d: &Dense| d.bias.len();
        if layers[1].inputs != rows(&layers[0]) || layers[2].inputs != rows(&layers[1]) {
            return Err(PolicyError::Shape("layers do not chain"));
        }
        let features = layers[0].inputs;
        let actions = rows(&layers[2]);
        require(&st, "features", &features.to_string(), "features")?;
        require(&st, "outputs", &actions.to_string(), "outputs")?;
        require(
            &st,
            "hidden",
            &format!("{},{}", rows(&layers[0]), rows(&layers[1])),
            "hidden",
        )?;
        let inverse = st
            .metadata("inv_scales")
            .ok_or(PolicyError::Scales)?
            .split(',')
            .map(|part| match part.parse::<f32>() {
                Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
                _ => Err(PolicyError::Scales),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if inverse.len() != features {
            return Err(PolicyError::Scales);
        }
        Ok(Self {
            inverse,
            layers,
            a: Vec::new(),
            b: Vec::new(),
        })
    }

    #[must_use]
    pub fn features(&self) -> usize {
        self.inverse.len()
    }

    #[must_use]
    pub fn actions(&self) -> usize {
        self.layers[2].bias.len()
    }

    pub fn logits(&mut self, features: &[f32]) -> &[f32] {
        self.a.clear();
        self.a.extend(
            (0..self.inverse.len())
                .map(|i| features.get(i).copied().unwrap_or(0.0) * self.inverse[i]),
        );
        let [first, second, last] = &self.layers;
        first.forward(&self.a, &mut self.b, true);
        second.forward(&self.b, &mut self.a, true);
        last.forward(&self.a, &mut self.b, false);
        &self.b
    }

    pub fn probabilities(&mut self, features: &[f32], legal: &[u8], out: &mut Vec<f32>) {
        let logits = self.logits(features).to_vec();
        masked_softmax(&logits, legal, out);
    }
}
