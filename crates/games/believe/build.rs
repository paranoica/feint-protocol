use std::path::Path;

fn main() {
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let dir = Path::new("checkpoints");
    println!("cargo:rerun-if-changed=checkpoints");
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .expect("the checkpoints folder exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".safetensors.hex"))
        .collect();
    names.sort();
    for path in names {
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).expect("a checkpoint is hex text");
        let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        assert!(
            digits.len().is_multiple_of(2),
            "{}: odd hex",
            path.display()
        );
        let value = |c: u8| match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("{}: not lowercase hex", path.display()),
        };
        let bytes: Vec<u8> = digits
            .chunks_exact(2)
            .map(|pair| value(pair[0]) * 16 + value(pair[1]))
            .collect();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".hex"))
            .expect("a checkpoint file name");
        std::fs::write(Path::new(&out).join(name), bytes).expect("OUT_DIR is writable");
    }
}
