use jenkins::utils::version_compare;
// cargo test --test test_version_compare -- --nocapture

#[test]
fn test_version_compare() {
  let result = version_compare("2.5.1", "3.0.0", "<");
  println!("Comparing versions 2.5.1 and 3.0.0 with '<': {}", result);
  assert!(result);

  let result = version_compare("3.6.1", "3.0.0", ">");
  println!("Comparing versions 3.6.1 and 3.0.0 with '>': {}", result);
  assert!(result);

  let result = version_compare("3.6.1", "3.6.1", "==");
  println!("Comparing versions 3.6.1 and 3.6.1 with '==': {}", result);
  assert!(result);

  let result = version_compare("3.6", "3.6.4", "<");
  println!("Comparing versions 3.6 and 3.6.4 with '<': {}", result);
  assert!(result);

  let result = version_compare("3.6.4", "3.6.4", "==");
  println!("Comparing versions 3.6.4 and 3.6.4 with '==': {}", result);
  assert!(result);

  let result = version_compare("3.6.4", "3.6.4", ">=");
  println!("Comparing versions 3.6.4 and 3.6.4 with '>=': {}", result);
  assert!(result);

  let result = version_compare("3.6.4", "3.6.1", ">=");
  println!("Comparing versions 3.6.4 and 3.6.1 with '>=': {}", result);
  assert!(result);

  let result = version_compare("3.6.1", "3.6.4", "<=");
  println!("Comparing versions 3.6.1 and 3.6.4 with '<=': {}", result);
  assert!(result);
}
