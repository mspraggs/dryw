// not initializer
// 0
class Foo {
  init(arg) {
    print "Foo.init(" + arg + ")";
    this.field = "init";
  }
}
fun init() {
  print "not initializer";
}
init();