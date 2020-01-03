use super::*;

use proptest::strategy::Strategy;

#[test]
fn without_list_or_bitstring_returns_false() {
    run(
        file!(),
        |arc_process| {
            (
                strategy::term::is_list(arc_process.clone()),
                strategy::term(arc_process.clone())
                    .prop_filter("Right cannot be a list or bitstring", |right| {
                        !(right.is_list() || right.is_bitstring())
                    }),
            )
        },
        |(left, right)| {
            prop_assert_eq!(native(left, right), false.into());

            Ok(())
        },
    );
}

#[test]
fn with_empty_list_right_returns_false() {
    is_equal_or_less_than(|_, _| Term::NIL, false);
}

#[test]
fn with_lesser_list_right_returns_false() {
    is_equal_or_less_than(
        |_, process| {
            process
                .cons(process.integer(0).unwrap(), process.integer(0).unwrap())
                .unwrap()
        },
        false,
    );
}

#[test]
fn with_same_list_right_returns_true() {
    is_equal_or_less_than(|left, _| left, true);
}

#[test]
fn with_same_value_list_right_returns_true() {
    is_equal_or_less_than(
        |_, process| {
            process
                .cons(process.integer(0).unwrap(), process.integer(1).unwrap())
                .unwrap()
        },
        true,
    );
}

#[test]
fn with_greater_list_right_returns_true() {
    is_equal_or_less_than(
        |_, process| {
            process
                .cons(process.integer(0).unwrap(), process.integer(2).unwrap())
                .unwrap()
        },
        true,
    );
}

#[test]
fn with_bitstring_right_returns_true() {
    run(
        file!(),
        |arc_process| {
            (
                strategy::term::is_list(arc_process.clone()),
                strategy::term::is_bitstring(arc_process.clone()),
            )
        },
        |(left, right)| {
            prop_assert_eq!(native(left, right), true.into());

            Ok(())
        },
    );
}

fn is_equal_or_less_than<R>(right: R, expected: bool)
where
    R: FnOnce(Term, &Process) -> Term,
{
    super::is_equal_or_less_than(
        |process| {
            process
                .cons(process.integer(0).unwrap(), process.integer(1).unwrap())
                .unwrap()
        },
        right,
        expected,
    );
}
