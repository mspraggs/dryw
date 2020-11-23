// [0, 1, 1, 4, 9, 25, 64, 169, 441, 1156]
// 0
class Foo < Iter {
    fn __init__(self, max) {
        self.first = 0;
        self.second = 1;
        self.count = 0;
        self.max = max;
    }

    fn __iter__(self) {
        return self;
    }

    fn __next__(self) {
        if self.count == self.max {
            return sentinel();
        }
        self.count += 1;
        var old_first = self.first;
        self.first = self.second;
        self.second = old_first + self.first;
        return old_first;
    }
}

fn square(n) { return n * n; }

print(Foo(10).map(square).collect());