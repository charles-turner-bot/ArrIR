use serde::{Deserialize, Serialize};

/// The logical type of a variable's values (`plans/schema-and-values.md`).
///
/// It describes the values operations receive, never storage byte order, a codec or
/// GDAL's working type. Complex, text, time and other families need their own contracts
/// before they get a variant.
///
/// The variants are flat (`Float32`, not `Float(W32)`) so the wire tag reads as the
/// dtype's name, as in `plans/dag.md`'s wire example.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DType {
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
}

/// Whether a variable's dtype is known when the schema is written.
///
/// An unknown dtype is a fact still to be supplied, not a numeric fallback. It allows a
/// structurally valid plan but blocks operations that need the dtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeKnowledge {
    Known(DType),
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtypes_use_their_names_as_wire_tags() {
        let known = TypeKnowledge::Known(DType::Float32);
        assert_eq!(
            serde_json::to_string(&known).unwrap(),
            r#"{"Known":"Float32"}"#
        );
        assert_eq!(
            serde_json::to_string(&TypeKnowledge::Unknown).unwrap(),
            r#""Unknown""#
        );
        assert_eq!(
            serde_json::from_str::<TypeKnowledge>(r#"{"Known":"UInt64"}"#).unwrap(),
            TypeKnowledge::Known(DType::UInt64)
        );
        assert!(serde_json::from_str::<DType>(r#""Complex64""#).is_err());
    }
}
