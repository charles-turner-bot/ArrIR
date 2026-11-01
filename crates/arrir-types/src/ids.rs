use serde::{Deserialize, Serialize};

/// A dimension symbol, not a coordinate label or an array position.
///
/// It is plan-wide and names named axes only; positional axes carry no `DimId`. Two
/// axes with the same `DimId` are the same axis. That asserts nothing about their
/// extents or labels, which are checked separately (`plans/dag.md`).
///
/// A newtype keeps dimensions distinct from variables even when spelled alike.
/// ```compile_fail
/// use arrir_types::{DimId, VarId};
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
/// It is unique within one node's schema, not across the plan (`plans/dag.md`).
///
/// A coordinate variable and its dimension may have the same string name;
/// their distinct types prevent accidental substitution in Rust APIs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VarId(String);

impl VarId {
    /// Accept any string symbol, as [`DimId::new`] does.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the symbol.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A plan-local name for an external dataset, used as `Open { binding, schema }`.
///
/// Each binding is opened by at most one live node. It is not a URL or a host pointer,
/// and decoding it resolves nothing: the host supplies the data at bind time
/// (`plans/design.md`, `plans/dag.md`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BindingId(String);

impl BindingId {
    /// Accept any string symbol, as [`DimId::new`] does.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the symbol.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A grid symbol. Variables refer to a grid by this ID through their placement, so
/// several variables can share one grid (`plans/grids.md`).
///
/// It is scoped to one node's schema. Two inputs share a grid only when their grid
/// definitions are equal, never because their IDs match (`plans/dag.md`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GridId(String);

impl GridId {
    /// Accept any string symbol, as [`DimId::new`] does.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the symbol.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn ids_preserve_host_symbols_without_normalisation() {
        for name in ["", "time", "Time", " latitude ", "温度"] {
            assert_eq!(DimId::new(name).as_str(), name);
            assert_eq!(VarId::new(name).as_str(), name);
            assert_eq!(BindingId::new(name).as_str(), name);
            assert_eq!(GridId::new(name).as_str(), name);
        }
        assert_ne!(DimId::new("time"), DimId::new("Time"));
    }

    #[test]
    fn ids_round_trip_as_strings() {
        let dimension = DimId::new("time");
        let variable = VarId::new("time");
        let binding = BindingId::new("weights");
        let grid = GridId::new("t_grid");
        assert_eq!(serde_json::to_string(&dimension).unwrap(), r#""time""#);
        assert_eq!(serde_json::to_string(&variable).unwrap(), r#""time""#);
        assert_eq!(serde_json::to_string(&binding).unwrap(), r#""weights""#);
        assert_eq!(serde_json::to_string(&grid).unwrap(), r#""t_grid""#);
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
        assert_eq!(serde_json::from_str::<GridId>(r#""t_grid""#).unwrap(), grid);
        assert!(serde_json::from_str::<BindingId>("42").is_err());
    }

    #[test]
    fn dimension_sets_serialise_in_a_deterministic_order() {
        let dimensions = BTreeSet::from([DimId::new("time"), DimId::new("lat")]);
        let encoded = serde_json::to_string(&dimensions).unwrap();
        assert_eq!(encoded, r#"["lat","time"]"#);
        assert_eq!(
            serde_json::from_str::<BTreeSet<DimId>>(&encoded).unwrap(),
            dimensions
        );
    }
}
