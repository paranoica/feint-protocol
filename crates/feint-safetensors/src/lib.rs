pub const MAX_HEADER: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeTensorsError {
    Truncated,
    HeaderTooLarge,
    Utf8,
    Json(&'static str),
    Dtype,
    Offsets,
    Layout,
    Unaligned,
    Shape,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tensor<'a> {
    name: String,
    shape: Vec<usize>,
    bytes: &'a [u8],
}

impl Tensor<'_> {
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    pub fn values(&self) -> Vec<f32> {
        self.bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeTensors<'a> {
    tensors: Vec<Tensor<'a>>,
    metadata: Vec<(String, String)>,
}

struct RawTensor {
    name: String,
    dtype: String,
    shape: Vec<u64>,
    start: u64,
    end: u64,
}

struct Reader<'h> {
    b: &'h [u8],
    i: usize,
}

type Res<T> = Result<T, SafeTensorsError>;

fn bad<T>(reason: &'static str) -> Res<T> {
    Err(SafeTensorsError::Json(reason))
}

impl Reader<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Res<()> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            bad("unexpected character")
        }
    }

    fn string(&mut self) -> Res<String> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.i) else {
                return bad("unterminated string");
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else {
                        return bad("unterminated string");
                    };
                    self.i += 1;
                    out.push(match e {
                        b'"' | b'\\' | b'/' => e,
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        _ => return bad("unsupported escape"),
                    });
                }
                0..=0x1f => return bad("control character in string"),
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| SafeTensorsError::Utf8)
    }

    fn number(&mut self) -> Res<u64> {
        self.ws();
        let start = self.i;
        let mut n: u64 = 0;
        while let Some(&c @ b'0'..=b'9') = self.b.get(self.i) {
            n = n
                .checked_mul(10)
                .and_then(|n| n.checked_add(u64::from(c - b'0')))
                .map_or_else(|| bad("number overflows"), Ok)?;
            self.i += 1;
        }
        let digits = self.i - start;
        if digits == 0 {
            return bad("expected a non-negative integer");
        }
        if digits > 1 && self.b[start] == b'0' {
            return bad("leading zero");
        }
        Ok(n)
    }

    fn numbers(&mut self) -> Res<Vec<u64>> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(out);
        }
        loop {
            out.push(self.number()?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return bad("malformed array"),
            }
        }
    }

    fn members(&mut self, mut entry: impl FnMut(&mut Self, String) -> Res<()>) -> Res<()> {
        self.expect(b'{')?;
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(());
        }
        loop {
            let key = self.string()?;
            self.expect(b':')?;
            entry(self, key)?;
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return bad("malformed object"),
            }
        }
    }

    fn tensor(&mut self, name: String) -> Res<RawTensor> {
        let mut dtype = None;
        let mut shape = None;
        let mut offsets = None;
        self.members(|r, key| {
            match key.as_str() {
                "dtype" if dtype.is_none() => dtype = Some(r.string()?),
                "shape" if shape.is_none() => shape = Some(r.numbers()?),
                "data_offsets" if offsets.is_none() => offsets = Some(r.numbers()?),
                _ => return bad("unexpected or repeated tensor key"),
            }
            Ok(())
        })?;
        let (Some(dtype), Some(shape), Some(offsets)) = (dtype, shape, offsets) else {
            return bad("missing tensor key");
        };
        let [start, end] = offsets[..] else {
            return bad("data_offsets needs two numbers");
        };
        Ok(RawTensor {
            name,
            dtype,
            shape,
            start,
            end,
        })
    }
}

fn has_duplicate(mut names: Vec<&str>) -> bool {
    names.sort_unstable();
    names.windows(2).any(|w| w[0] == w[1])
}

impl<'a> SafeTensors<'a> {
    pub fn parse(bytes: &'a [u8]) -> Res<SafeTensors<'a>> {
        let Some(prefix) = bytes.first_chunk::<8>() else {
            return Err(SafeTensorsError::Truncated);
        };
        let n = u64::from_le_bytes(*prefix);
        if n > MAX_HEADER as u64 {
            return Err(SafeTensorsError::HeaderTooLarge);
        }
        let n = n as usize;
        let Some(data_at) = 8usize.checked_add(n).filter(|&e| n > 0 && e <= bytes.len()) else {
            return Err(SafeTensorsError::Truncated);
        };
        let header = std::str::from_utf8(&bytes[8..data_at]).map_err(|_| SafeTensorsError::Utf8)?;
        let data = &bytes[data_at..];

        let mut reader = Reader {
            b: header.as_bytes(),
            i: 0,
        };
        let mut raw = Vec::new();
        let mut metadata: Vec<(String, String)> = Vec::new();
        let mut saw_metadata = false;
        reader.members(|r, key| {
            if key == "__metadata__" {
                if saw_metadata {
                    return Err(SafeTensorsError::Duplicate);
                }
                saw_metadata = true;
                r.members(|r, k| {
                    let v = r.string()?;
                    metadata.push((k, v));
                    Ok(())
                })
            } else {
                raw.push(r.tensor(key)?);
                Ok(())
            }
        })?;
        if reader.peek().is_some() {
            return bad("trailing characters");
        }

        if has_duplicate(raw.iter().map(|t| t.name.as_str()).collect())
            || has_duplicate(metadata.iter().map(|(k, _)| k.as_str()).collect())
        {
            return Err(SafeTensorsError::Duplicate);
        }

        let mut placed = Vec::with_capacity(raw.len());
        for t in raw {
            if t.dtype != "F32" {
                return Err(SafeTensorsError::Dtype);
            }
            if t.end < t.start {
                return Err(SafeTensorsError::Offsets);
            }
            let mut elements: u64 = 1;
            for &d in &t.shape {
                elements = elements.checked_mul(d).ok_or(SafeTensorsError::Shape)?;
            }
            if elements.checked_mul(4) != Some(t.end - t.start) {
                return Err(SafeTensorsError::Shape);
            }
            if t.end > data.len() as u64 {
                return Err(SafeTensorsError::Offsets);
            }
            if t.start % 4 != 0 {
                return Err(SafeTensorsError::Unaligned);
            }
            let shape = t
                .shape
                .iter()
                .map(|&d| usize::try_from(d).map_err(|_| SafeTensorsError::Shape))
                .collect::<Res<Vec<_>>>()?;
            placed.push((t.start as usize, t.end as usize, t.name, shape));
        }

        placed.sort_unstable_by_key(|p| (p.0, p.1));
        let mut cursor = 0;
        let mut tensors = Vec::with_capacity(placed.len());
        for (start, end, name, shape) in placed {
            if start != cursor {
                return Err(SafeTensorsError::Layout);
            }
            cursor = end;
            tensors.push(Tensor {
                name,
                shape,
                bytes: &data[start..end],
            });
        }
        if cursor != data.len() {
            return Err(SafeTensorsError::Layout);
        }
        Ok(SafeTensors { tensors, metadata })
    }

    pub fn tensor(&self, name: &str) -> Option<&Tensor<'a>> {
        self.tensors.iter().find(|t| t.name == name)
    }

    pub fn metadata(&self, key: &str) -> Option<&str> {
        self.metadata
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}
