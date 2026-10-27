#[allow(dead_code)]
fn dead_helper_1(x: i64) -> i64 {
    x * 3 + 42
}

#[allow(dead_code)]
fn dead_helper_2(x: i64) -> i64 {
    dead_helper_1(x) * 2
}

fn compute(n: i64) -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < n {
        let dead_val: i64 = (i * 17 + 4) / 3;
        let _dead_val2: i64 = dead_val * dead_val + 1;
        sum = (sum + i) % 1000000007;
        i += 1;
    }
    sum
}

fn main() {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < 1000 {
        total = (total + compute(10000)) % 1000000007;
        i += 1;
    }
    println!("{}", total);
}
