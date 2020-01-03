use proptest::prop_assert_eq;
use proptest::strategy::{Just, Strategy};

use crate::otp::erlang::tuple_size_1::native;
use crate::test::{run, strategy};

#[test]
fn without_tuple_errors_badarg() {
    run(
        file!(),
        |arc_process| {
            (
                Just(arc_process.clone()),
                strategy::term::is_not_tuple(arc_process),
            )
        },
        |(arc_process, tuple)| {
            prop_assert_is_not_tuple!(native(&arc_process, tuple), tuple);

            Ok(())
        },
    );
}

#[test]
fn with_tuple_returns_arity() {
    run(
        file!(),
        |arc_process| {
            (Just(arc_process.clone()), 0_usize..=3_usize).prop_flat_map(|(arc_process, size)| {
                (
                    Just(arc_process.clone()),
                    Just(size),
                    strategy::term::tuple::intermediate(
                        strategy::term(arc_process.clone()),
                        (size..=size).into(),
                        arc_process.clone(),
                    ),
                )
            })
        },
        |(arc_process, size, term)| {
            prop_assert_eq!(
                native(&arc_process, term),
                Ok(arc_process.integer(size).unwrap())
            );

            Ok(())
        },
    );
}
