fn compute_lsr(_scale: i64, iters: i64) -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < iters {
        let term = i * 9;
        sum += term;
        i += 1;
    }
    sum
}

fn main() {
    let res = compute_lsr(9, 50000000);
    println!("{}", res);
}
