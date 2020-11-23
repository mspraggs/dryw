// A
// 0
class A {
  fn say(self) {
    print("A");
  }
}
class B < A {
  fn getClosure(self) {
    fn closure() {
      super.say();
    }
    return closure;
  }
  fn say(self) {
    print("B");
  }
}
class C < B {
  fn say(self) {
    print("C");
  }
}
C().getClosure()();
