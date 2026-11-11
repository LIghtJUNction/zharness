use zharness::{Tool, ToolSet, tools::{FixedRegistry, Registry}, zcall::v1::TokenStorage};
fn main() -> Result<(), zharness::Error> {
    let descriptors = [Tool::new("fs.read", "Read with application authorization", r#"{"type":"object"}"#), Tool::new("math.add", "Add numbers", r#"{"type":"object"}"#)];
    let tools = ToolSet::new(&descriptors)?;
    let mut index = [0; 2]; let indexed = tools.indexed(&mut index)?;
    assert!(indexed.get("math.add").is_some());
    let mut slots = [None; 4]; let mut dynamic = FixedRegistry::new(&mut slots);
    dynamic.insert(descriptors[0], &mut TokenStorage::<32>::new())?;
    assert!(dynamic.find("fs.read").is_some());
    println!("linear={}, index_bytes={}, fixed_bytes={}", tools.len(), indexed.memory_bytes(), dynamic.memory_bytes());
    Ok(())
}
