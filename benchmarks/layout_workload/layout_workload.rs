fn classify(x: i64) -> i64 {
    if x % 2 == 0 {
        x / 2
    } else if x % 3 == 0 {
        x / 3 + 1
    } else if x % 5 == 0 {
        x / 5 + 2
    } else {
        x + 7
    }
}

fn main() {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 10000000 {
        sum = (sum + classify(i)) % 1000000007;
        i += 1;
    }
    println!("{}", sum);
}
