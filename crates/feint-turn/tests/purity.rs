use std::fs;
use std::path::Path;

const GAME_WORDS: [&str; 10] = [
    "hockey", "puck", "mallet", "rink", "poker", "kuhn", "chess", "tennis", "believe", "deck",
];

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs" || e == "toml") {
            out.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

#[test]
fn core_names_no_game() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    files.push((
        "Cargo.toml".into(),
        fs::read_to_string(root.join("Cargo.toml")).unwrap(),
    ));
    assert!(files.len() >= 2, "found only {} files", files.len());
    for (name, text) in &files {
        let lower = text.to_lowercase();
        for word in GAME_WORDS {
            assert!(!lower.contains(word), "{name} names a game: {word:?}");
        }
    }
}
