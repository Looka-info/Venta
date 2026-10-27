fn compute_pressure(x: i64) -> i64 {
    let v1 = x + 1;
    let v2 = x + 2;
    let v3 = x + 3;
    let v4 = x + 4;
    let v5 = x + 5;
    let v6 = x + 6;
    let v7 = x + 7;
    let v8 = x + 8;
    let v9 = x + 9;
    let v10 = x + 10;
    (v1 * v2) - (v3 * v4) + (v5 * v6) - (v7 * v8) + (v9 * v10)
        + (v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10)
}

fn main() {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 5000000 {
        sum += compute_pressure(i % 50);
        i += 1;
    }
    println!("{}", sum);
}
