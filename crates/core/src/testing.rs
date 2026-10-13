//! A small property-test harness: [`check`] and [`check_sized`].
//!
//! A property test generates many random inputs from a seeded [`Pcg32`] and
//! checks that a property holds for each one:
//!
//! ```
//! use nadir_core::testing::check;
//!
//! check(
//!     "addition commutes",
//!     200,
//!     |rng| (rng.range(-1000..1000), rng.range(-1000..1000)),
//!     |&(a, b)| if a + b == b + a { Ok(()) } else { Err(format!("{a} + {b}")) },
//! );
//! ```
//!
//! Runs are reproducible. The seed is [`DEFAULT_SEED`] unless the
//! `NADIR_SEED` environment variable is set, and a failure panics with the
//! seed, the case number, the generated input and the property's message.
//! Run the test again with that seed to get the same failure.

use std::any::Any;
use std::fmt::{self, Debug};
use std::panic::{self, AssertUnwindSafe};

use crate::hash::fnv1a64;
use crate::rng::Pcg32;

/// The environment variable that overrides the seed: a `u64`, in decimal or
/// in hex with a `0x` prefix.
pub const SEED_VAR: &str = "NADIR_SEED";

/// The seed used when [`SEED_VAR`] is not set, so that ordinary test runs
/// are reproducible.
pub const DEFAULT_SEED: u64 = 0x4e41_4449_5220_0001;

/// The largest size [`check_sized`] passes to a generator.
pub const MAX_SIZE: u32 = 100;

/// Checks that `property` holds for `cases` inputs made by `generate`.
///
/// `property` returns `Err` with an explanation when it does not hold; it
/// may also simply panic. Each case draws from its own random stream, so a
/// case's input does not depend on how much randomness earlier cases used.
///
/// # Panics
///
/// Panics on the first failing case, with the seed and case number needed
/// to reproduce it. Also panics if `NADIR_SEED` is set but is not a `u64`.
pub fn check<T: Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    let run = Run {
        name,
        seed: seed_from_env(),
        cases,
    };
    if let Err(failure) = run.unsized_cases(&generate, &property) {
        panic!("{failure}");
    }
}

/// Like [`check`], for inputs that have a natural size, such as a list of
/// operations.
///
/// `generate` also receives a size between 0 and [`MAX_SIZE`], and should
/// produce an input about that large: for example a list of that many
/// operations. Sizes grow over the run, from 0 in the first case to
/// `MAX_SIZE` in the last.
///
/// When a case fails, the harness tries the same case again at every
/// smaller size and reports the smallest size that still fails. This is all
/// the shrinking there is, so it helps only if a smaller size gives a
/// simpler input.
///
/// # Panics
///
/// As for [`check`].
pub fn check_sized<T: Debug>(
    name: &str,
    cases: u32,
    generate: impl Fn(&mut Pcg32, u32) -> T,
    property: impl Fn(&T) -> Result<(), String>,
) {
    let run = Run {
        name,
        seed: seed_from_env(),
        cases,
    };
    if let Err(failure) = run.sized_cases(&generate, &property) {
        panic!("{failure}");
    }
}

/// One invocation of [`check`] or [`check_sized`].
struct Run<'a> {
    name: &'a str,
    seed: u64,
    cases: u32,
}

impl Run<'_> {
    fn unsized_cases<T: Debug>(
        &self,
        generate: &impl Fn(&mut Pcg32) -> T,
        property: &impl Fn(&T) -> Result<(), String>,
    ) -> Result<(), Failure> {
        for case in 0..self.cases {
            self.run_case(case, generate, property)
                .map_err(|(input, message)| self.failure(case, None, input, message))?;
        }
        Ok(())
    }

    fn sized_cases<T: Debug>(
        &self,
        generate: &impl Fn(&mut Pcg32, u32) -> T,
        property: &impl Fn(&T) -> Result<(), String>,
    ) -> Result<(), Failure> {
        for case in 0..self.cases {
            let size = self.size_of_case(case);
            let Err(mut found) = self.run_case(case, |rng| generate(rng, size), property) else {
                continue;
            };
            // Shrink: the smallest size at which this same case still fails.
            let mut shrunk_to = size;
            for smaller in 0..size {
                if let Err(simpler) = self.run_case(case, |rng| generate(rng, smaller), property) {
                    (found, shrunk_to) = (simpler, smaller);
                    break;
                }
            }
            let sizes = Sizes {
                failed_at: size,
                shrunk_to,
            };
            return Err(self.failure(case, Some(sizes), found.0, found.1));
        }
        Ok(())
    }

    /// Sizes ramp linearly from 0 in the first case to `MAX_SIZE` in the
    /// last.
    fn size_of_case(&self, case: u32) -> u32 {
        match self.cases.saturating_sub(1) {
            0 => MAX_SIZE,
            last => (u64::from(case) * u64::from(MAX_SIZE) / u64::from(last)) as u32,
        }
    }

    /// The generator for one case. It depends only on the seed, the
    /// property's name and the case number.
    fn case_rng(&self, case: u32) -> Pcg32 {
        Pcg32::new(self.seed ^ fnv1a64(self.name.as_bytes()), u64::from(case))
    }

    /// Runs one case. On failure, returns the input (as `Debug` text) and
    /// the reason.
    fn run_case<T: Debug>(
        &self,
        case: u32,
        generate: impl FnOnce(&mut Pcg32) -> T,
        property: &impl Fn(&T) -> Result<(), String>,
    ) -> Result<(), (String, String)> {
        // A panic is a failure like any other, and needs the seed in its
        // report just as much. The closures are not used again after a
        // panic, so observing broken state through them is not a concern.
        let mut rng = self.case_rng(case);
        let input =
            panic::catch_unwind(AssertUnwindSafe(|| generate(&mut rng))).map_err(|payload| {
                let message = format!("the generator panicked: {}", panic_message(&*payload));
                ("<not generated>".to_string(), message)
            })?;
        match panic::catch_unwind(AssertUnwindSafe(|| property(&input))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err((format!("{input:?}"), message)),
            Err(payload) => Err((
                format!("{input:?}"),
                format!("panicked: {}", panic_message(&*payload)),
            )),
        }
    }

    fn failure(&self, case: u32, sizes: Option<Sizes>, input: String, message: String) -> Failure {
        Failure {
            name: self.name.to_string(),
            seed: self.seed,
            case,
            cases: self.cases,
            sizes,
            input,
            message,
        }
    }
}

/// Everything needed to understand and reproduce a failing case.
#[derive(Debug, PartialEq, Eq)]
struct Failure {
    name: String,
    seed: u64,
    case: u32,
    cases: u32,
    sizes: Option<Sizes>,
    input: String,
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sizes {
    /// The size at which the case first failed.
    failed_at: u32,
    /// The smallest size at which it still fails. `input` is from this one.
    shrunk_to: u32,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            name,
            seed,
            case,
            cases,
            sizes,
            input,
            message,
        } = self;
        writeln!(
            f,
            "property `{name}` failed at case {case} of {cases} (seed {seed:#018x})"
        )?;
        writeln!(f, "  error: {message}")?;
        writeln!(f, "  input: {input}")?;
        if let Some(Sizes {
            failed_at,
            shrunk_to,
        }) = sizes
        {
            writeln!(f, "  size: {shrunk_to} (first failed at size {failed_at})")?;
        }
        write!(
            f,
            "  to reproduce, run the test again with {SEED_VAR}={seed:#x}"
        )
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "<a panic payload that is not a string>"
    }
}

fn seed_from_env() -> u64 {
    match std::env::var(SEED_VAR) {
        Ok(text) => match parse_seed(&text) {
            Some(seed) => seed,
            None => panic!(
                "{SEED_VAR} must be a u64, in decimal or as 0x-prefixed hex, but it is {text:?}"
            ),
        },
        Err(std::env::VarError::NotPresent) => DEFAULT_SEED,
        Err(std::env::VarError::NotUnicode(_)) => panic!("{SEED_VAR} is not valid Unicode"),
    }
}

fn parse_seed(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const SEED: u64 = 0x1234_5678_9abc_def0;

    fn run(name: &str, cases: u32) -> Run<'_> {
        Run {
            name,
            seed: SEED,
            cases,
        }
    }

    fn below_900(n: &u32) -> Result<(), String> {
        if *n < 900 {
            Ok(())
        } else {
            Err(format!("{n} is not below 900"))
        }
    }

    fn up_to_1000(rng: &mut Pcg32) -> u32 {
        rng.range(0..1000u32)
    }

    /// A list of `size` small numbers.
    fn numbers(rng: &mut Pcg32, size: u32) -> Vec<u32> {
        (0..size).map(|_| rng.range(0..10u32)).collect()
    }

    /// The property for the sized tests: a list must have fewer than five
    /// elements. Returned as a closure because the harness hands the
    /// property a `&Vec<u32>`, the generator's exact output type.
    fn shorter_than_5() -> impl Fn(&Vec<u32>) -> Result<(), String> {
        |list| {
            if list.len() < 5 {
                Ok(())
            } else {
                Err(format!("{} elements", list.len()))
            }
        }
    }

    fn panic_text(result: std::thread::Result<()>) -> String {
        let payload = result.expect_err("check should have panicked");
        panic_message(&*payload).to_string()
    }

    #[test]
    fn a_passing_property_runs_every_case() {
        let count = Cell::new(0);
        check(
            "counts cases",
            250,
            |rng| rng.next_u32(),
            |_| {
                count.set(count.get() + 1);
                Ok(())
            },
        );
        assert_eq!(count.get(), 250);

        count.set(0);
        check_sized(
            "counts sized cases",
            40,
            |rng, _| rng.next_u32(),
            |_| {
                count.set(count.get() + 1);
                Ok(())
            },
        );
        assert_eq!(count.get(), 40);
    }

    #[test]
    fn zero_cases_pass_without_running_anything() {
        check(
            "never runs",
            0,
            |_| panic!("generated"),
            |_: &()| Err("checked".to_string()),
        );
        check_sized(
            "never runs",
            0,
            |_, _| panic!("generated"),
            |_: &()| Err("checked".to_string()),
        );
    }

    #[test]
    fn a_failure_reports_the_seed_case_input_and_message() {
        let failure = run("below 900", 500)
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("some case is >= 900");
        assert_eq!(failure.name, "below 900");
        assert_eq!(failure.seed, SEED);
        assert_eq!(failure.cases, 500);
        assert_eq!(failure.sizes, None);
        let input: u32 = failure.input.parse().expect("the input is a number");
        assert!(input >= 900);
        assert_eq!(failure.message, format!("{input} is not below 900"));

        let text = failure.to_string();
        let case = failure.case;
        assert_eq!(
            text,
            format!(
                "property `below 900` failed at case {case} of 500 (seed 0x123456789abcdef0)\n  \
                 error: {input} is not below 900\n  \
                 input: {input}\n  \
                 to reproduce, run the test again with NADIR_SEED=0x123456789abcdef0"
            )
        );
    }

    #[test]
    fn the_reported_seed_and_case_reproduce_the_failure() {
        let first = run("below 900", 500)
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("fails");

        // Running again with the same seed fails identically.
        let again = Run {
            name: "below 900",
            seed: first.seed,
            cases: 500,
        };
        assert_eq!(again.unsized_cases(&up_to_1000, &below_900), Err(first));

        // The failing input can be regenerated from the seed and case alone.
        let first = again
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("fails");
        let regenerated = up_to_1000(&mut again.case_rng(first.case));
        assert_eq!(regenerated.to_string(), first.input);
        // It was the first failing case: every earlier one passes.
        for case in 0..first.case {
            assert!(below_900(&up_to_1000(&mut again.case_rng(case))).is_ok());
        }
    }

    #[test]
    fn different_seeds_and_names_give_different_inputs() {
        let inputs = |run: Run<'_>| -> Vec<u32> {
            (0..20).map(|case| run.case_rng(case).next_u32()).collect()
        };
        let base = inputs(run("a", 20));
        assert_eq!(base, inputs(run("a", 20)));
        assert_ne!(base, inputs(run("b", 20)));
        assert_ne!(
            base,
            inputs(Run {
                name: "a",
                seed: SEED + 1,
                cases: 20
            })
        );
        // Cases are independent streams, not one sequence.
        let mut sorted = base.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 20);
    }

    #[test]
    fn check_panics_with_the_report() {
        let text = panic_text(panic::catch_unwind(|| {
            check("below 900", 500, up_to_1000, below_900)
        }));
        let seed = seed_from_env();
        assert!(
            text.starts_with("property `below 900` failed at case "),
            "{text}"
        );
        assert!(text.contains(&format!("(seed {seed:#018x})")), "{text}");
        assert!(text.contains("is not below 900"), "{text}");
        assert!(text.ends_with(&format!("NADIR_SEED={seed:#x}")), "{text}");

        // The same seed gives the same report, word for word.
        let again = panic_text(panic::catch_unwind(|| {
            check("below 900", 500, up_to_1000, below_900)
        }));
        assert_eq!(text, again);
    }

    #[test]
    fn a_panicking_property_is_reported_with_its_seed_and_input() {
        let explode = |n: &u32| -> Result<(), String> {
            assert!(*n < 900, "index {n} out of range");
            Ok(())
        };
        let failure = run("panics", 500)
            .unsized_cases(&up_to_1000, &explode)
            .expect_err("some case panics");
        let input: u32 = failure.input.parse().expect("the input is a number");
        assert!(input >= 900);
        assert_eq!(
            failure.message,
            format!("panicked: index {input} out of range")
        );
        assert_eq!(failure.seed, SEED);
    }

    #[test]
    fn a_panicking_generator_is_reported() {
        let generate = |rng: &mut Pcg32| -> u32 {
            let n = rng.range(0..1000u32);
            assert!(n < 900, "cannot generate {n}");
            n
        };
        let failure = run("generator", 500)
            .unsized_cases(&generate, &|_: &u32| Ok(()))
            .expect_err("the generator panics");
        assert_eq!(failure.input, "<not generated>");
        assert!(
            failure
                .message
                .starts_with("the generator panicked: cannot generate 9"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn non_string_panics_are_reported_too() {
        let explode = |_: &u32| -> Result<(), String> { std::panic::panic_any(42) };
        let failure = run("odd panic", 3)
            .unsized_cases(&up_to_1000, &explode)
            .expect_err("panics");
        assert_eq!(failure.case, 0);
        assert_eq!(
            failure.message,
            "panicked: <a panic payload that is not a string>"
        );
    }

    #[test]
    fn sizes_ramp_from_zero_to_the_maximum() {
        let sizes = |cases: u32| -> Vec<u32> {
            (0..cases)
                .map(|case| run("sizes", cases).size_of_case(case))
                .collect()
        };
        assert_eq!(sizes(1), [MAX_SIZE]);
        assert_eq!(sizes(2), [0, MAX_SIZE]);
        assert_eq!(sizes(5), [0, 25, 50, 75, 100]);
        let many = sizes(1000);
        assert_eq!((many[0], many[999]), (0, MAX_SIZE));
        assert!(many.is_sorted());
        // Every size is used when there are enough cases.
        let mut distinct = many.clone();
        distinct.dedup();
        assert_eq!(distinct.len(), MAX_SIZE as usize + 1);
        // The multiplication must not overflow for a huge run.
        assert_eq!(run("sizes", u32::MAX).size_of_case(u32::MAX - 1), MAX_SIZE);
    }

    #[test]
    fn the_generator_receives_the_size_of_its_case() {
        let seen = Cell::new(0);
        let run = run("sees sizes", 101);
        let generate = |_: &mut Pcg32, size: u32| size;
        let property = |&size: &u32| {
            seen.set(seen.get() + 1);
            if size == seen.get() - 1 {
                Ok(())
            } else {
                Err(format!("case {} got size {size}", seen.get() - 1))
            }
        };
        assert_eq!(run.sized_cases(&generate, &property), Ok(()));
        assert_eq!(seen.get(), 101);
    }

    #[test]
    fn a_sized_failure_is_shrunk_to_the_smallest_failing_size() {
        let failure = run("short lists", 101)
            .sized_cases(&numbers, &shorter_than_5())
            .expect_err("long lists fail");
        // Sizes 0 to 4 pass, so case 5 (size 5) is the first to fail, and
        // nothing smaller fails.
        assert_eq!(failure.case, 5);
        assert_eq!(
            failure.sizes,
            Some(Sizes {
                failed_at: 5,
                shrunk_to: 5
            })
        );
        assert_eq!(failure.message, "5 elements");

        // With few cases the first failure is at a large size, and shrinks.
        let failure = run("short lists", 3)
            .sized_cases(&numbers, &shorter_than_5())
            .expect_err("long lists fail");
        assert_eq!(failure.case, 1);
        assert_eq!(
            failure.sizes,
            Some(Sizes {
                failed_at: 50,
                shrunk_to: 5
            })
        );
        assert_eq!(failure.message, "5 elements");
        // The reported input is the shrunk one, regenerated from the same
        // case at the smaller size.
        let regenerated = numbers(
            &mut Run {
                name: "short lists",
                seed: SEED,
                cases: 3,
            }
            .case_rng(1),
            5,
        );
        assert_eq!(failure.input, format!("{regenerated:?}"));

        let text = failure.to_string();
        assert!(
            text.contains("\n  size: 5 (first failed at size 50)\n"),
            "{text}"
        );
        assert!(
            text.contains(&format!("\n  input: {regenerated:?}\n")),
            "{text}"
        );
    }

    #[test]
    fn check_sized_panics_with_the_shrunk_report() {
        let text = panic_text(panic::catch_unwind(|| {
            check_sized("short lists", 3, numbers, shorter_than_5())
        }));
        assert!(
            text.starts_with("property `short lists` failed at case 1 of 3 (seed 0x"),
            "{text}"
        );
        assert!(text.contains("\n  error: 5 elements\n"), "{text}");
        assert!(
            text.contains("\n  size: 5 (first failed at size 50)\n"),
            "{text}"
        );
    }

    #[test]
    fn seeds_parse_as_decimal_or_hex() {
        assert_eq!(parse_seed("0"), Some(0));
        assert_eq!(parse_seed("12345"), Some(12345));
        assert_eq!(parse_seed(" 42\n"), Some(42));
        assert_eq!(parse_seed("0xff"), Some(255));
        assert_eq!(parse_seed("0XFF"), Some(255));
        assert_eq!(parse_seed("0x4e41444952200001"), Some(DEFAULT_SEED));
        assert_eq!(parse_seed("18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_seed("0xffffffffffffffff"), Some(u64::MAX));
        for bad in [
            "",
            "0x",
            "seed",
            "-1",
            "1.5",
            "18446744073709551616",
            "0x1_0000_0000_0000_0000",
            "ff",
        ] {
            assert_eq!(parse_seed(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_printed_seed_parses_back() {
        let failure = run("below 900", 500)
            .unsized_cases(&up_to_1000, &below_900)
            .expect_err("fails");
        let text = failure.to_string();
        let printed = text
            .rsplit_once(&format!("{SEED_VAR}="))
            .expect("the report names the variable")
            .1;
        assert_eq!(parse_seed(printed), Some(SEED));
    }
}
