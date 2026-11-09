macro_rules! make {
    ($t:ident { $($field:ident : $value:expr),* $(,)? }) => {{
        let mut built = $t::default();
        $(built.$field = $value;)*
        built
    }};
}
