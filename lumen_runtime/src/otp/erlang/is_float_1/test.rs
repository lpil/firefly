use proptest::prop_assert_eq;

use crate::otp::erlang::is_float_1::native;
use crate::test::{run, strategy};

#[test]
fn without_float_returns_false() {
    run(
        file!(),
        |arc_process| strategy::term::is_not_float(arc_process.clone()),
        |term| {
            prop_assert_eq!(native(term), false.into());

            Ok(())
        },
    );
}

#[test]
fn with_float_returns_true() {
    run(
        file!(),
        |arc_process| strategy::term::float(arc_process.clone()),
        |term| {
            prop_assert_eq!(native(term), true.into());

            Ok(())
        },
    );
}
