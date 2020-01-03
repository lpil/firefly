use proptest::arbitrary::any;
use proptest::prop_assert_eq;
use proptest::strategy::{Just, Strategy};

use crate::otp::erlang::binary_to_float_1::native;
use crate::test::{run, strategy};

#[test]
fn without_binary_errors_badarg() {
    run(
        file!(),
        |arc_process| {
            (
                Just(arc_process.clone()),
                strategy::term::is_not_binary(arc_process.clone()),
            )
        },
        |(arc_process, binary)| {
            prop_assert_badarg!(
                native(&arc_process, binary),
                format!("binary ({}) must be a binary", binary)
            );

            Ok(())
        },
    );
}

#[test]
fn with_binary_with_integer_errors_badarg() {
    run(
        file!(),
        |arc_process| {
            (Just(arc_process.clone()), any::<isize>()).prop_flat_map(|(arc_process, integer)| {
                (
                    Just(arc_process.clone()),
                    strategy::term::binary::containing_bytes(
                        integer.to_string().as_bytes().to_owned(),
                        arc_process.clone(),
                    ),
                )
            })
        },
        |(arc_process, binary)| {
            prop_assert_badarg!(
                native(&arc_process, binary),
                format!("float string ({}) does not contain decimal point", binary)
            );

            Ok(())
        },
    );
}

#[test]
fn with_binary_with_f64_returns_floats() {
    run(
        file!(),
        |arc_process| {
            (Just(arc_process.clone()), any::<f64>()).prop_flat_map(|(arc_process, f)| {
                let byte_vec = format!("{:?}", f).as_bytes().to_owned();

                (
                    Just(arc_process.clone()),
                    Just(f),
                    strategy::term::binary::containing_bytes(byte_vec, arc_process.clone()),
                )
            })
        },
        |(arc_process, f, binary)| {
            prop_assert_eq!(
                native(&arc_process, binary),
                Ok(arc_process.float(f).unwrap())
            );

            Ok(())
        },
    );
}

#[test]
fn with_binary_with_less_than_min_f64_errors_badarg() {
    run(
        file!(),
        |arc_process| {
            (Just(arc_process.clone()), strategy::term::binary::containing_bytes("-1797693134862315700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0".as_bytes().to_owned(), arc_process.clone()))
        },
        |(arc_process, binary)| {
            prop_assert_badarg!(
                native(&arc_process, binary),
                format!("Erlang does not support infinities")
            );

            Ok(())
        },
    );
}

#[test]
fn with_binary_with_greater_than_max_f64_errors_badarg() {
    run(
        file!(),
        |arc_process| {
            (Just(arc_process.clone()), strategy::term::binary::containing_bytes("1797693134862315700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0".as_bytes().to_owned(), arc_process.clone()))
        },
        |(arc_process, binary)| {
            prop_assert_badarg!(
                native(&arc_process, binary),
                format!("Erlang does not support infinities")
            );

            Ok(())
        },
    );
}
