//! Telling a card number from a handle that stands in for one.

/// Whether a value is a card number by shape.
///
/// Twelve to nineteen digits and nothing else, passing the Luhn check.
///
/// # What this is for, and what it is not
///
/// No type in this workspace can hold a card number, and every adapter takes
/// payments in a way that never sends one through the caller's process. This
/// catches the remaining case: a field wired to the wrong source, where a
/// caller passes a PAN somewhere a saved-instrument handle belongs and the
/// digits would go out in a request body — or, worse, in a URL, and from there
/// into every proxy log on the path.
///
/// **It proves nothing about security on its own.** It is a shape test. A card
/// number with a space in it is not caught, and a handle that happens to be
/// twelve Luhn-valid digits is refused although it is not a card. Both are the
/// right way round: the cost of the first is a guard that did not fire on a
/// mistake nobody makes, and the cost of the second is an error message.
///
/// # Why it lives here
///
/// It was written twice, byte for byte, in `kasapay-stripe` and
/// `kasapay-iyzico`, each under its own private name, with one crate's copy
/// pointing at the other's in a comment. A rule the whole workspace rests on
/// is not two implementations that nothing keeps in step — and an adapter
/// written outside this repository has the same obligation, so it needs the
/// same test rather than a third copy of it.
///
/// ```
/// use kasapay_core::looks_like_a_card_number;
///
/// assert!(looks_like_a_card_number("4242424242424242"));
/// assert!(!looks_like_a_card_number("pm_1Pgc75B7WZ01zgkWlHVgdEGJ"));
/// assert!(!looks_like_a_card_number("mdt_uDPFVsxjR4"));
/// // Luhn is what separates a card from any long number.
/// assert!(!looks_like_a_card_number("4242424242424243"));
/// ```
#[must_use]
pub fn looks_like_a_card_number(value: &str) -> bool {
    let digits = value.as_bytes();
    if !(12..=19).contains(&digits.len()) || !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(place, digit)| {
            let value = u32::from(*digit - b'0');
            if place % 2 == 0 {
                value
            } else if value > 4 {
                value * 2 - 9
            } else {
                value * 2
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::looks_like_a_card_number;

    /// The four brands' own published test numbers, which is external ground
    /// truth rather than this function's own arithmetic restated.
    #[test]
    fn the_published_test_cards_are_recognised() {
        for number in [
            "4242424242424242", // Visa, Stripe's
            "5528790000000008", // Mastercard, iyzico's
            "378282246310005",  // American Express, 15 digits
            "6011111111111117", // Discover
            "3530111333300000", // JCB
            "36227206271667",   // Diners Club, 14 digits
        ] {
            assert!(looks_like_a_card_number(number), "{number}");
        }
    }

    #[test]
    fn the_handles_every_adapter_here_actually_passes_are_not_card_numbers() {
        for handle in [
            "pm_1Pgc75B7WZ01zgkWlHVgdEGJ",
            "mdt_uDPFVsxjR4",
            "card-token-1",
            "cus_kasapay1",
            "cst_tKt44u85MM",
            "3C679366HH908993F",
            "12345678",
        ] {
            assert!(!looks_like_a_card_number(handle), "{handle}");
        }
    }

    #[test]
    fn shape_alone_is_not_enough() {
        // Right length, all digits, fails Luhn.
        assert!(!looks_like_a_card_number("4242424242424243"));
        // Luhn-valid but too short to be a card, and too long to be one.
        assert!(!looks_like_a_card_number("18"));
        assert!(!looks_like_a_card_number("55287900000"));
        assert!(!looks_like_a_card_number("55287900000000081234"));
        // A hex token of card-number length is not digits.
        assert!(!looks_like_a_card_number(
            "8f2c1c5d4e6a4b0f9d3a7c1e2b5f8a04"
        ));
        // Digits with anything else in them are not caught, which is stated.
        assert!(!looks_like_a_card_number("4242 4242 4242 4242"));
    }
}
