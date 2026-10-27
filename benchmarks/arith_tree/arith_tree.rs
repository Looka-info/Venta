fn eval_tree(x: i64, y: i64) -> i64 {
    let a = (x + y) * (x - y) + (x * 3) - (y * 5);
    let b = (a + x) * 2 - (a - y) * 3 + (x + y);
    let c = (a + b) + (a * 2) - (b * 2) + 7;
    c
}

fn main() {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 2000000 {
        sum += eval_tree(i % 100, (i + 3) % 100);
        i += 1;
    }
    println!("{}", sum);
}
