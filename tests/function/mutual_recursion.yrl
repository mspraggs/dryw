// true
// true
// 0
fn isEven(n) {
  if n == 0 { return true; }
  return isOdd(n - 1);
}
fn isOdd(n) {
  return isEven(n - 1);
}
print(isEven(4));
print(isOdd(3));
