mod with_false_left;
mod with_true_left;

use crate::otp::erlang::xor_2::native;
use crate::test::{run, strategy};

#[test]
fn without_boolean_left_errors_badarg() {
    crate::test::without_boolean_left_errors_badarg(file!(), native);
}
