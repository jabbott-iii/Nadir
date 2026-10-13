//! The user's X resources, of which we read one: `Xft.dpi`.
//!
//! Desktop environments set `Xft.dpi` to say how dense the display is, and
//! toolkits scale their interfaces by it. 96 is the standard density.
//!
//! The resources are text, one `name: value` to a line. A line that starts
//! with `!` is a comment. If a name is set twice, the later line wins.

/// The density at which the scale factor is 1.
const STANDARD_DPI: f32 = 96.0;

/// The densities we believe. Anything outside is a broken setting.
const PLAUSIBLE_DPI: core::ops::RangeInclusive<f32> = 24.0..=1536.0;

/// Returns the scale factor that the resources ask for, or 1 if they do not
/// say or there are none.
pub(super) fn scale_factor(resources: Option<&str>) -> f32 {
    resources
        .and_then(xft_dpi)
        .map_or(1.0, |dpi| dpi / STANDARD_DPI)
}

/// Returns the value of `Xft.dpi`, if the resources set it to a density we
/// believe.
///
/// Only the exact name `Xft.dpi` counts. The loose forms that the resource
/// manager also accepts, such as `*dpi`, are not looked for.
fn xft_dpi(resources: &str) -> Option<f32> {
    let value = resources
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("Xft.dpi"))
        .filter_map(|rest| rest.trim_start().strip_prefix(':'))
        .next_back()?;
    let dpi: f32 = value.trim().parse().ok()?;
    PLAUSIBLE_DPI.contains(&dpi).then_some(dpi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dpi_is_read_from_its_line() {
        assert_eq!(xft_dpi("Xft.dpi:\t96"), Some(96.0));
        assert_eq!(xft_dpi("Xft.dpi: 144\n"), Some(144.0));
        assert_eq!(xft_dpi("Xft.dpi:192"), Some(192.0));
        assert_eq!(xft_dpi("  Xft.dpi  :  120  "), Some(120.0));
        assert_eq!(xft_dpi("Xft.dpi:\t110.5"), Some(110.5));
    }

    #[test]
    fn the_dpi_is_found_among_other_resources() {
        let resources = "\
Xcursor.size:\t24
Xcursor.theme:\tAdwaita
Xft.antialias:\t1
Xft.dpi:\t168
Xft.hinting:\t1
Xft.rgba:\trgb
";
        assert_eq!(xft_dpi(resources), Some(168.0));
        assert_eq!(scale_factor(Some(resources)), 1.75);
    }

    #[test]
    fn the_later_of_two_lines_wins() {
        assert_eq!(xft_dpi("Xft.dpi: 96\nXft.dpi: 192\n"), Some(192.0));
        // A broken later line does not fall back to an earlier one, just as
        // it would not in the resource manager.
        assert_eq!(xft_dpi("Xft.dpi: 96\nXft.dpi: big\n"), None);
    }

    #[test]
    fn other_names_are_not_the_dpi() {
        for resources in [
            "",
            "Xft.dpix: 192",
            "Xft.dpi.other: 192",
            "MyXft.dpi: 192",
            "! Xft.dpi: 192",
            "*dpi: 192",
            "Xft*dpi: 192",
            "xft.dpi: 192",
            "Xft.dpi 192",
            "Xft.antialias: 1\nXcursor.size: 24",
        ] {
            assert_eq!(xft_dpi(resources), None, "{resources:?}");
        }
    }

    #[test]
    fn a_value_that_is_not_a_plausible_density_is_ignored() {
        for value in [
            "", "abc", "96dpi", "0", "-96", "23.9", "1537", "1e30", "inf", "NaN", "96 96",
        ] {
            let resources = format!("Xft.dpi: {value}");
            assert_eq!(xft_dpi(&resources), None, "{value:?}");
            assert_eq!(scale_factor(Some(&resources)), 1.0, "{value:?}");
        }
        assert_eq!(xft_dpi("Xft.dpi: 24"), Some(24.0));
        assert_eq!(xft_dpi("Xft.dpi: 1536"), Some(1536.0));
    }

    #[test]
    fn the_scale_factor_is_the_dpi_over_96() {
        assert_eq!(scale_factor(Some("Xft.dpi: 96")), 1.0);
        assert_eq!(scale_factor(Some("Xft.dpi: 144")), 1.5);
        assert_eq!(scale_factor(Some("Xft.dpi: 192")), 2.0);
        assert_eq!(scale_factor(Some("Xft.dpi: 72")), 0.75);
    }

    #[test]
    fn without_resources_or_a_dpi_the_scale_factor_is_one() {
        assert_eq!(scale_factor(None), 1.0);
        assert_eq!(scale_factor(Some("")), 1.0);
        assert_eq!(scale_factor(Some("Xcursor.size: 24")), 1.0);
    }
}
