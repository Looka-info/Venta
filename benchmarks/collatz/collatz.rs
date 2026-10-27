#[inline(never)]
fn collatz_len(mut n: i64) -> i64 {
    let mut steps: i64 = 0;
    while n > 1 {
        if n % 2 == 0 {
            n = n / 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
    }
    steps
}

fn main() {
    let mut total_steps: i64 = 0;
    let mut i: i64 = 1;
    while i <= 500000 {
        total_steps += collatz_len(i);
        i += 1;
    }
    println!("{}", total_steps);
}
