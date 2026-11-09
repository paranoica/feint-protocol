/// Build a generated `#[non_exhaustive]` protocol type: its default with the given fields.
macro_rules! make {
    ($t:ident { $($field:ident : $value:expr),* $(,)? }) => {{
        let mut built = $t::default();
        $(built.$field = $value;)*
        built
    }};
}
