use serde::{Deserialize, Serialize};

/// The length of one axis, as a declared schema states it (`plans/shapes.md`).
///
/// `Known(0)` is an empty axis, not an unknown one. Shape inference (#2) will add a
/// `Symbolic` variant for derived schemas; a declared schema never holds one.
///
/// On the wire a known extent is a decimal string, `{"Known":"365"}`, so JavaScript
/// never rounds it through a floating-point `Number` (`plans/schema-and-values.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Extent {
    Known(#[serde(with = "decimal_string")] u64),
    Unknown,
}

/// Write a `u64` as a canonical decimal string, and read only that form back.
///
/// Canonical means no sign, no leading zeros and no whitespace, so each number has
/// exactly one spelling and two hosts write the same bytes.
mod decimal_string {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        let value: u64 = text
            .parse()
            .map_err(|_| D::Error::custom(format!("not a decimal integer: {text:?}")))?;
        if value.to_string() != text {
            return Err(D::Error::custom(format!(
                "not a canonical decimal integer: {text:?}"
            )));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extents_are_decimal_strings() {
        let big = Extent::Known(u64::MAX);
        let encoded = serde_json::to_string(&big).unwrap();
        assert_eq!(encoded, r#"{"Known":"18446744073709551615"}"#);
        assert_eq!(serde_json::from_str::<Extent>(&encoded).unwrap(), big);
        assert_eq!(
            serde_json::to_string(&Extent::Unknown).unwrap(),
            r#""Unknown""#
        );
    }

    #[test]
    fn non_canonical_extents_are_rejected() {
        for text in [
            r#"{"Known":365}"#,
            r#"{"Known":"0365"}"#,
            r#"{"Known":"+365"}"#,
            r#"{"Known":"-1"}"#,
            r#"{"Known":" 365"}"#,
            r#"{"Known":""}"#,
            r#"{"Known":"18446744073709551616"}"#,
        ] {
            assert!(serde_json::from_str::<Extent>(text).is_err(), "{text}");
        }
        assert_eq!(
            serde_json::from_str::<Extent>(r#"{"Known":"0"}"#).unwrap(),
            Extent::Known(0)
        );
    }
}
