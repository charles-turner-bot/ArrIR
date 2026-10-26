use arrir::{DimId, Dimension, Operation, Plan, Shape};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = Shape::new(vec![
        Dimension::new(DimId::new("time")?, 2),
        Dimension::new(DimId::new("lat")?, 3),
    ])?;
    let plan = Plan::new(
        input,
        vec![Operation::Ravel {
            dimension: DimId::new("sample")?,
        }],
    )?;
    plan.validate()?;
    assert_eq!(plan.infer_shape()?.dimensions[0].length, 6);
    Ok(())
}
