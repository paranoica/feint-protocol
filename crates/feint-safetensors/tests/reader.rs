use feint_safetensors::{MAX_HEADER, SafeTensors, SafeTensorsError};

const PYTHON_FILE: &[u8] = include_bytes!("../../feint-ml/checkpoints/airhockey-g999.safetensors");

fn file(header: &str, data: &[u8]) -> Vec<u8> {
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(data);
    out
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn valid() -> Vec<u8> {
    let header = r#"{"__metadata__":{"k":"v","n":"2"},"a":{"dtype":"F32","shape":[2],"data_offsets":[0,8]},"b":{"dtype":"F32","shape":[1,2],"data_offsets":[8,16]}}"#;
    file(header, &floats(&[1.5, -0.0, 3.0, -4.25]))
}

fn refused(bytes: &[u8]) -> SafeTensorsError {
    match SafeTensors::parse(bytes) {
        Ok(_) => panic!("a hostile file was accepted"),
        Err(e) => e,
    }
}

#[test]
fn a_valid_file_gives_its_tensors_shapes_values_and_metadata() {
    let bytes = valid();
    let st = SafeTensors::parse(&bytes).unwrap();
    let a = st.tensor("a").unwrap();
    assert_eq!(a.shape(), &[2]);
    assert_eq!(
        a.values().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        [1.5f32.to_bits(), (-0.0f32).to_bits()]
    );
    let b = st.tensor("b").unwrap();
    assert_eq!(b.shape(), &[1, 2]);
    assert_eq!(b.values(), vec![3.0, -4.25]);
    assert_eq!(st.metadata("k"), Some("v"));
    assert_eq!(st.metadata("n"), Some("2"));
    assert_eq!(st.metadata("missing"), None);
    assert!(st.tensor("c").is_none());
    assert!(st.tensor("__metadata__").is_none());
}

#[test]
fn whitespace_and_key_order_inside_the_header_do_not_matter() {
    let header = "{ \"x\" : { \"shape\" : [ 1 ] ,\n\t\"data_offsets\" : [ 0 , 4 ] , \"dtype\" : \"F32\" } }   ";
    let bytes = file(header, &floats(&[7.0]));
    let st = SafeTensors::parse(&bytes).unwrap();
    assert_eq!(st.tensor("x").unwrap().values(), vec![7.0]);
}

#[test]
fn a_file_without_metadata_or_tensors_is_valid_and_empty() {
    let bytes = file("{}", &[]);
    let st = SafeTensors::parse(&bytes).unwrap();
    assert!(st.tensor("a").is_none());
    assert_eq!(st.metadata("k"), None);
}

#[test]
fn reads_a_file_written_by_python() {
    let st = SafeTensors::parse(PYTHON_FILE).unwrap();
    let shapes = [
        ("layer0.weight", vec![256, 18]),
        ("layer0.bias", vec![256]),
        ("layer1.weight", vec![256, 256]),
        ("layer1.bias", vec![256]),
        ("layer2.weight", vec![2, 256]),
        ("layer2.bias", vec![2]),
    ];
    for (name, shape) in shapes {
        let t = st.tensor(name).unwrap();
        assert_eq!(t.shape(), shape.as_slice(), "{name}");
        assert_eq!(t.values().len(), shape.iter().product::<usize>(), "{name}");
    }
    assert_eq!(st.metadata("format"), Some("feint-mlp-v1"));
    assert_eq!(st.metadata("activation"), Some("relu"));
    assert_eq!(st.metadata("features"), Some("18"));
    assert!(st.tensor("scales").is_none());
}

#[test]
fn every_truncation_of_a_valid_file_is_an_error_and_never_a_panic() {
    let bytes = valid();
    for len in 0..bytes.len() {
        assert!(
            SafeTensors::parse(&bytes[..len]).is_err(),
            "prefix of {len} bytes"
        );
    }
}

#[test]
fn rejects_truncated_header() {
    assert_eq!(refused(&[1, 0, 0]), SafeTensorsError::Truncated);
}

#[test]
fn rejects_header_longer_than_the_file() {
    let mut bytes = valid();
    let len = (bytes.len() as u64).to_le_bytes();
    bytes[..8].copy_from_slice(&len);
    assert_eq!(refused(&bytes), SafeTensorsError::Truncated);
}

#[test]
fn rejects_header_above_the_cap() {
    let mut bytes = ((MAX_HEADER as u64) + 1).to_le_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(b' ', MAX_HEADER + 1));
    assert_eq!(refused(&bytes), SafeTensorsError::HeaderTooLarge);
    let mut huge = u64::MAX.to_le_bytes().to_vec();
    huge.extend_from_slice(b"{}");
    assert_eq!(refused(&huge), SafeTensorsError::HeaderTooLarge);
}

#[test]
fn rejects_non_utf8_header() {
    let mut bytes = file("{\"a\":1}", &[]);
    bytes[10] = 0xff;
    assert_eq!(refused(&bytes), SafeTensorsError::Utf8);
}

#[test]
fn rejects_offsets_past_the_end() {
    let header = r#"{"a":{"dtype":"F32","shape":[2],"data_offsets":[0,8]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0]))),
        SafeTensorsError::Offsets
    );
}

#[test]
fn rejects_offsets_that_overflow() {
    let header = r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,18446744073709551616]}}"#;
    assert!(matches!(
        refused(&file(header, &floats(&[1.0]))),
        SafeTensorsError::Json(_) | SafeTensorsError::Offsets
    ));
    let header = r#"{"a":{"dtype":"F32","shape":[4611686018427387904,4],"data_offsets":[0,4]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0]))),
        SafeTensorsError::Shape
    );
    let header = r#"{"a":{"dtype":"F32","shape":[3,12297829382473034411],"data_offsets":[0,4]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0]))),
        SafeTensorsError::Shape
    );
}

#[test]
fn rejects_end_before_start() {
    let header = r#"{"a":{"dtype":"F32","shape":[0],"data_offsets":[4,0]},"b":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0]))),
        SafeTensorsError::Offsets
    );
}

#[test]
fn rejects_unaligned_offsets() {
    let header = r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[2,6]}}"#;
    let mut data = vec![0, 0];
    data.extend(floats(&[1.0]));
    assert_eq!(refused(&file(header, &data)), SafeTensorsError::Unaligned);
}

#[test]
fn rejects_overlapping_tensors() {
    let header = r#"{"a":{"dtype":"F32","shape":[2],"data_offsets":[0,8]},"b":{"dtype":"F32","shape":[2],"data_offsets":[4,12]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0, 2.0, 3.0]))),
        SafeTensorsError::Layout
    );
}

#[test]
fn rejects_gaps_and_bytes_after_the_last_tensor() {
    let header = r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]},"b":{"dtype":"F32","shape":[1],"data_offsets":[8,12]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0, 2.0, 3.0]))),
        SafeTensorsError::Layout
    );
    let header = r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0, 2.0]))),
        SafeTensorsError::Layout
    );
}

#[test]
fn rejects_non_f32() {
    for dtype in ["F16", "BF16", "F64", "I32", "U8", "BOOL", "f32"] {
        let header = format!(r#"{{"a":{{"dtype":"{dtype}","shape":[1],"data_offsets":[0,4]}}}}"#);
        assert_eq!(
            refused(&file(&header, &floats(&[1.0]))),
            SafeTensorsError::Dtype,
            "{dtype}"
        );
    }
}

#[test]
fn rejects_shape_that_disagrees_with_the_bytes() {
    let header = r#"{"a":{"dtype":"F32","shape":[3],"data_offsets":[0,8]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0, 2.0]))),
        SafeTensorsError::Shape
    );
}

#[test]
fn rejects_duplicate_names() {
    let header = r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]},"a":{"dtype":"F32","shape":[1],"data_offsets":[4,8]}}"#;
    assert_eq!(
        refused(&file(header, &floats(&[1.0, 2.0]))),
        SafeTensorsError::Duplicate
    );
    let header = r#"{"__metadata__":{"k":"v","k":"w"}}"#;
    assert_eq!(refused(&file(header, &[])), SafeTensorsError::Duplicate);
}

#[test]
fn rejects_deep_nesting_and_unexpected_shapes_of_json() {
    let hostile = [
        r#"{"a":{"dtype":{"x":"F32"},"shape":[1],"data_offsets":[0,4]}}"#,
        r#"{"a":{"dtype":"F32","shape":[[1]],"data_offsets":[0,4]}}"#,
        r#"{"__metadata__":{"k":{"deep":"v"}}}"#,
        r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4],"extra":1}}"#,
        r#"{"a":{"dtype":"F32","dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
        r#"{"a":{"dtype":"F32","shape":[1],"shape":[1],"data_offsets":[0,4]}}"#,
        r#"{"a":{"dtype":"F32","shape":[1]}}"#,
        r#"{"a":{"dtype":"F32","shape":[-1],"data_offsets":[0,4]}}"#,
        r#"{"a":{"dtype":"F32","shape":[1.5],"data_offsets":[0,4]}}"#,
        r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4,8]}}"#,
        r#"[]"#,
        r#"{"a":1}"#,
        r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}"#,
        r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}} x"#,
        r#"{"a":{"dtype":"F32","shape":[1],"data_offsets":[0,4]},}"#,
        r#"{"a\u0000":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
        "",
    ];
    for header in hostile {
        assert!(
            SafeTensors::parse(&file(header, &floats(&[1.0]))).is_err(),
            "{header}"
        );
    }
    let mut deep = String::from(r#"{"__metadata__":"#);
    deep.push_str(&"[".repeat(100_000));
    let bytes = file(&deep, &[]);
    assert!(SafeTensors::parse(&bytes).is_err());
}

#[test]
fn metadata_strings_keep_their_escapes() {
    let header = r#"{"__metadata__":{"q":"a\"b\\c\/d\n"}}"#;
    let bytes = file(header, &[]);
    let st = SafeTensors::parse(&bytes).unwrap();
    assert_eq!(st.metadata("q"), Some("a\"b\\c/d\n"));
}
