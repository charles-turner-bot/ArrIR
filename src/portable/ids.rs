use serde::{Deserialize, Serialize};

/// A dimension symbol, not a coordinate label or an array position.
///
/// A newtype keeps dimensions distinct from variables even when spelled alike.
/// ```compile_fail
/// use arrir::portable::{DimId, VarId};
/// let dimension: DimId = VarId::new("time");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DimId(String);

impl DimId {
    /// Accept any string symbol, including an empty host name.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the symbol without allocating another string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A variable symbol. Coordinate membership will be a separate schema role.
///
/// A coordinate variable and its dimension may have the same string name;
/// their distinct types prevent accidental substitution in Rust APIs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VarId(String);

impl VarId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A plan-local symbol for externally supplied data, such as weights or a mask.
///
/// This is not a URL or a host pointer. Decoding it performs no lookup. A future
/// adapter must check the binding's declared requirements before using its data.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BindingId(String);

impl BindingId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn ids_preserve_host_symbols_without_normalisation() {
        for name in ["", "time", "Time", " latitude ", "温度"] {
            assert_eq!(DimId::new(name).as_str(), name);
            assert_eq!(VarId::new(name).as_str(), name);
            assert_eq!(BindingId::new(name).as_str(), name);
        }
        assert_ne!(DimId::new("time"), DimId::new("Time"));
    }

    #[test]
    fn ids_round_trip_as_strings() {
        let dimension = DimId::new("time");
        let variable = VarId::new("time");
        let binding = BindingId::new("weights");
        assert_eq!(serde_json::to_string(&dimension).unwrap(), r#""time""#);
        assert_eq!(serde_json::to_string(&variable).unwrap(), r#""time""#);
        assert_eq!(serde_json::to_string(&binding).unwrap(), r#""weights""#);
        assert_eq!(
            serde_json::from_str::<DimId>(r#""time""#).unwrap(),
            dimension
        );
        assert_eq!(
            serde_json::from_str::<VarId>(r#""time""#).unwrap(),
            variable
        );
        assert_eq!(
            serde_json::from_str::<BindingId>(r#""weights""#).unwrap(),
            binding
        );
        assert!(serde_json::from_str::<BindingId>("42").is_err());
    }

    #[test]
    fn dimension_symbols_work_as_deterministic_map_keys() {
        let dimensions = BTreeMap::from([(DimId::new("time"), 2_u64)]);
        let encoded = serde_json::to_string(&dimensions).unwrap();
        assert_eq!(encoded, r#"{"time":2}"#);
        assert_eq!(
            serde_json::from_str::<BTreeMap<DimId, u64>>(&encoded).unwrap(),
            dimensions
        );
    }
}
