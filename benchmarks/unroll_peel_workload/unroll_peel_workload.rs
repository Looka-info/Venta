fn dot4(acc: i64) -> i64 {
    let mut total: i64 = acc;
    let mut k: i64 = 0;
    while k < 4 {
        total = total + (k * 3 + 1);
        k = k + 1;
    }
    total
}

fn main() {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 10000000 {
        sum = dot4(sum);
        i = i + 1;
    }
    println!("{}", sum);
}
