use std::borrow::Cow;

use cookie::{Cookie as RawCookie, ParseError};
use time::{Date, Month, OffsetDateTime, Time};

pub(crate) fn parse_cookie<'a>(
    input: impl Into<Cow<'a, str>>,
) -> Result<RawCookie<'a>, ParseError> {
    let input = input.into();
    let expires = input
        .split(';')
        .skip(1)
        .filter_map(|attribute| {
            let (name, value) = attribute.split_once('=').unwrap_or((attribute, ""));
            name.trim()
                .eq_ignore_ascii_case("expires")
                .then_some(value.trim())
        })
        .last()
        .and_then(|value| parse_cookie_date(value.as_bytes()));
    let mut cookie = RawCookie::parse(input)?;
    // The dependency recognizes fixed HTTP-date formats. Browser cookie dates
    // use token parsing instead, with the last Expires attribute taking effect.
    cookie.unset_expires();
    if let Some(expires) = expires {
        cookie.set_expires(expires);
    }
    Ok(cookie)
}

// Follows Chromium's ParseCookieExpirationTime: ASCII token delimiters,
// numeric prefixes, and the first occurrence of each date field. Timezone
// tokens are ignored; cookie expiration times are always interpreted as UTC.
fn parse_cookie_date(input: &[u8]) -> Option<OffsetDateTime> {
    const MONTHS: [&[u8; 3]; 12] = [
        b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov",
        b"dec",
    ];
    let (mut day, mut month, mut year, mut time) = (None, None, None, None);
    for token in input
        .split(|byte| matches!(byte, b'\t' | b' '..=b'/' | b';'..=b'@' | b'['..=b'`' | b'{'..=b'~'))
        .filter(|token| !token.is_empty())
    {
        if !token[0].is_ascii_digit() {
            if month.is_none() {
                month = MONTHS
                    .iter()
                    .position(|name| {
                        token
                            .get(..3)
                            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(*name))
                    })
                    .and_then(|index| Month::try_from(index as u8 + 1).ok());
            }
        } else if token.contains(&b':') {
            if time.is_none() {
                time = parse_time(token);
            }
        } else if day.is_none() && token.len() <= 2 {
            day = parse_number(token);
        } else if year.is_none() && token.len() <= 5 {
            year = parse_number(token);
        }
    }
    let year = match year? {
        year @ 0..=69 => year + 2000,
        year @ 70..=99 => year + 1900,
        year => year,
    };
    let day = u8::try_from(day?).ok()?;
    let month = month?;
    let [hour, minute, second] = time?;
    let time = Time::from_hms(hour, minute, second).ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }
    if year > 9999 {
        return Some(
            Date::from_calendar_date(9999, Month::December, 31)
                .ok()?
                .with_time(Time::MAX)
                .assume_utc(),
        );
    }
    Some(
        Date::from_calendar_date(year as i32, month, day)
            .ok()?
            .with_time(time)
            .assume_utc(),
    )
}

fn parse_number(input: &[u8]) -> Option<u32> {
    if !input.first()?.is_ascii_digit() {
        return None;
    }
    input
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .try_fold(0_u32, |number, byte| {
            number.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
        })
}

fn parse_time(mut input: &[u8]) -> Option<[u8; 3]> {
    let mut time = [0; 3];
    for (index, field) in time.iter_mut().enumerate() {
        let digits = input
            .iter()
            .take(2)
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        *field = u8::try_from(parse_number(&input[..digits])?).ok()?;
        input = &input[digits..];
        if index < 2 {
            input = input.strip_prefix(b":")?;
        }
    }
    Some(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_date() -> OffsetDateTime {
        Date::from_calendar_date(2037, Month::June, 9)
            .unwrap()
            .with_hms(19, 21, 5)
            .unwrap()
            .assume_utc()
    }

    #[test]
    fn browser_date_formats_share_the_same_utc_expiration() {
        for input in [
            "Tue, 09 Jun 2037 19:21:05 GMT",
            "Tuesday, 09-Jun-37 19:21:05 GMT",
            "Tue Jun  9 19:21:05 2037",
            "Tue, 9-Jun-2037 19:21:05 GMT",
            "Tue, 09 Jun 2037 19:21:05 UTC",
            "Tue, 09 Jun 2037 19:21:05 utc",
            "Tue, 09 Jun 2037 19:21:05",
            "Tue Jun 09 2037 19:21:05 GMT+0900 (日本標準時)",
            "Tue, 09 Jun 2037 19:21:05 GMT (😀)",
            "2037 June 9 19:21:05",
            "19:21:05 2037 Jun 9",
            "9 Jun 2037 ::00 19:21:05",
            "9 Jun 2037 19:21:05 01:02:03",
            "9 Jun 2037 19:21:05:00",
            "9 Jun 2037 256:00:00 19:21:05",
            "9 Jun 2037 19:21:05日本",
            "9x Jun 2037 19:21:05",
            "9 Jun 2037x 19:21:05",
        ] {
            assert_eq!(
                parse_cookie_date(input.as_bytes()),
                Some(expected_date()),
                "{input:?}"
            );
        }
        assert_eq!(
            parse_cookie_date(b"9 Jun 2037 19:21:05 (\xff\xfe)"),
            Some(expected_date())
        );
    }

    #[test]
    fn malformed_or_incomplete_dates_are_ignored() {
        for input in [
            "",
            "é",
            "éé",
            "時)",
            "😀",
            "2026年10月11日 12時00分00秒 (日本標準時)",
            "9 Jun 2037",
            "9 2037 19:21:05",
            "Jun 2037 19:21:05",
            "9 Jun 19:21:05",
            "0 Jun 2037 19:21:05",
            "32 Jun 2037 19:21:05",
            "29 Feb 2037 19:21:05",
            "31 Apr 2037 19:21:05",
            "009 Jun 2037 19:21:05",
            "9 Jun 123456 19:21:05",
            "9 Jun 2037 24:00:00",
            "9 Jun 2037 19:60:05",
            "9 Jun 2037 19:21:60",
            "9 Jun 2037 19::05",
            "9 Jun 2037 19:21",
            "9 Jun 2037 019:21:05",
            "9 Jun 2037 19:021:05",
            "9 Jun 2037 99:00:00 19:21:05",
        ] {
            assert_eq!(parse_cookie_date(input.as_bytes()), None, "{input:?}");
        }
    }

    #[test]
    fn abbreviated_years_and_single_digit_time_fields_are_supported() {
        for (input, year) in [
            ("1 Jan 00 1:2:3", 2000),
            ("1 Jan 69 1:2:3", 2069),
            ("1 Jan 70 1:2:3", 1970),
            ("1 Jan 99 1:2:3", 1999),
        ] {
            let expected = Date::from_calendar_date(year, Month::January, 1)
                .unwrap()
                .with_hms(1, 2, 3)
                .unwrap()
                .assume_utc();
            assert_eq!(
                parse_cookie_date(input.as_bytes()),
                Some(expected),
                "{input:?}"
            );
        }
    }

    #[test]
    fn out_of_range_years_remain_persistent_dates() {
        assert_eq!(parse_cookie_date(b"1 Jan 1600 0:0:0").unwrap().year(), 1600);
        assert_eq!(
            parse_cookie_date(b"1 Jan 10000 0:0:0").unwrap(),
            Date::from_calendar_date(9999, Month::December, 31)
                .unwrap()
                .with_time(Time::MAX)
                .assume_utc()
        );
    }

    #[test]
    fn last_expires_attribute_is_authoritative_even_when_invalid() {
        for (suffix, expected) in [
            ("", Some(expected_date())),
            ("; Expires=invalid", None),
            ("; Expires", None),
            ("; Expires=", None),
            (
                "; eXpIrEs=9 Jun 2037 19:21:05 (日本標準時)",
                Some(expected_date()),
            ),
        ] {
            let input = format!("sid=value; Expires=Tue, 09 Jun 2037 19:21:05 GMT{suffix}");
            let cookie = parse_cookie(input).unwrap();
            assert_eq!(cookie.expires_datetime(), expected, "{suffix:?}");
            assert_eq!(cookie.name_value(), ("sid", "value"));
        }
    }
}
