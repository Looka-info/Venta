fn inner_accum(base: i64, limit: i64) -> i64 {
    let mut sum: i64 = 0;
    let mut j: i64 = 0;
    while j < limit {
        sum += base * 2 + j;
        j += 1;
    }
    sum
}

fn main() {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < 100000 {
        total += inner_accum(i % 100, 20) % 5000;
        i += 1;
    }
    println!("{}", total);
}
