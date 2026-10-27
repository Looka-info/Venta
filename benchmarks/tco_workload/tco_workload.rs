fn tail_sum(n: i64, acc: i64) -> i64 {
    if n == 0 {
        return acc;
    }
    tail_sum(n - 1, (acc + n) % 1000000007)
}

fn main() {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < 10000 {
        total = (total + tail_sum(1000, 0)) % 1000000007;
        i += 1;
    }
    println!("{}", total);
}
