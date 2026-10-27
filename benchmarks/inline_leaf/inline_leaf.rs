fn add3(a: i64, b: i64, c: i64) -> i64 {
    a + b + c
}

fn scale_add(x: i64, factor: i64, bias: i64) -> i64 {
    x * factor + bias
}

fn leaf_work(val: i64) -> i64 {
    let s1 = add3(val, 15, 27);
    let s2 = scale_add(s1, 3, 7);
    s2
}

fn main() {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 10000000 {
        sum += leaf_work(i % 500) % 1000;
        i += 1;
    }
    println!("{}", sum);
}
