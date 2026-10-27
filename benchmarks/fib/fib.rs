#[inline(never)]
fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn main() {
    let res = fib(38);
    println!("{}", res);
}
