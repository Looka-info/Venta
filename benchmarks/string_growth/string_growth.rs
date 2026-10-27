#[inline(never)]
fn make_string(n: i64) -> String {
    let mut s = String::from("base_");
    let mut i = 0;
    while i < n {
        s.push_str("chunk_");
        i += 1;
    }
    s
}

fn main() {
    let mut total_len = 0;
    let mut iter = 0;
    while iter < 2000 {
        let s = make_string(10);
        total_len += s.len();
        iter += 1;
    }
    println!("{}", total_len);
}
