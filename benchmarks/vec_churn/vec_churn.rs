#[inline(never)]
fn churn(count: i64) -> i64 {
    let mut v: Vec<i64> = Vec::new();
    let mut i = 0;
    while i < count {
        v.push(i);
        i += 1;
    }
    let mut sum: i64 = 0;
    let mut j = 0;
    while j < v.len() {
        sum += v[j];
        j += 1;
    }
    sum
}

fn main() {
    let mut total: i64 = 0;
    let mut iter = 0;
    while iter < 200 {
        total += churn(10000);
        iter += 1;
    }
    println!("{}", total);
}
