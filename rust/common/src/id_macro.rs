#[macro_export]
macro_rules! id_type {
    ($t:tt) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $t(pub i32);

        impl $t {
            pub const fn new(inner: i32) -> $t {
                Self(inner)
            }
        }
    };
}
