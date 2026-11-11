#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub name: &'static str,
    pub id: u64,
    pub bytes: &'static [u8],
}

pub const CHECKPOINTS: &[Checkpoint] = &[Checkpoint {
    name: "airhockey-focus",
    id: 0x1517_1d6c_57a0_caa7,
    bytes: include_bytes!("../checkpoints/airhockey-focus.safetensors"),
}];

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn lookup(id: u64) -> Option<&'static Checkpoint> {
    CHECKPOINTS.iter().find(|c| c.id == id)
}
