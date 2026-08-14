// https://www.unwoundstack.com/blog/integration-testing-rust-binaries.html
use libtest_mimic::{Arguments, Trial};

pub struct Test {
    pub name: &'static str,
    pub test_fn: fn(),
}

#[linkme::distributed_slice]
pub static TESTS: [Test];

macro_rules! register {
    ($name:ident) => {
        const _: () = {
            #[linkme::distributed_slice(testing::TESTS)]
            static TEST: testing::Test = testing::Test {
                name: stringify!($name),
                test_fn: $name,
            };
        };
    };
}
pub(crate) use register;

#[allow(unused_macros)]
macro_rules! test {
    (fn $name:ident $($tt:tt)*) => {
        fn $name $($tt)*
        testing::register!($name);
    };
}
pub(crate) use test;

#[allow(unused_macros)]
macro_rules! smol_test {
    (async fn $name:ident $($tt:tt)*) => {
        #[macro_rules_attribute::apply(smol_macros::main!)]
        async fn $name $($tt)*
        testing::register!($name);
    };
}
pub(crate) use smol_test;

#[allow(unused_macros)]
macro_rules! tokio_test {
    (async fn $name:ident $($tt:tt)*) => {
        #[tokio::main(flavor = "current_thread")]
        async fn $name $($tt)*
        testing::register!($name);
    };
}
pub(crate) use tokio_test;

pub fn main() {
    crossmist::init();
    let args = Arguments::from_args();
    let tests = TESTS
        .iter()
        .map(|test| Trial::test(test.name, || Ok((test.test_fn)())))
        .collect();
    libtest_mimic::run(&args, tests).exit();
}

macro_rules! setup {
    () => {
        #[allow(unused_imports)]
        use testing::{smol_test, test, tokio_test};
        fn main() {
            testing::main();
        }
    };
}
pub(crate) use setup;
