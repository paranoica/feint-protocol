#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub name: &'static str,
    pub id: u64,
    pub bytes: &'static [u8],
}

pub const CHECKPOINTS: &[Checkpoint] = &[
    Checkpoint {
        name: "airhockey-early",
        id: 0x11e0_e1c1_a096_28b6,
        bytes: include_bytes!("../checkpoints/airhockey-early.safetensors"),
    },
    Checkpoint {
        name: "airhockey-pilot",
        id: 0x3ac0_02e1_2274_846f,
        bytes: include_bytes!("../checkpoints/airhockey-pilot.safetensors"),
    },
    Checkpoint {
        name: "airhockey-focus",
        id: 0x1517_1d6c_57a0_caa7,
        bytes: include_bytes!("../checkpoints/airhockey-focus.safetensors"),
    },
];

pub const LEVELS: [u64; 5] = [
    0x11e0_e1c1_a096_28b6,
    0x3ac0_02e1_2274_846f,
    0x3ac0_02e1_2274_846f,
    0x3ac0_02e1_2274_846f,
    0x1517_1d6c_57a0_caa7,
];

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn lookup(id: u64) -> Option<&'static Checkpoint> {
    CHECKPOINTS.iter().find(|c| c.id == id)
}

pub fn for_level(level: u8) -> Option<&'static Checkpoint> {
    let index = usize::from(level).checked_sub(1)?;
    lookup(*LEVELS.get(index)?)
}
