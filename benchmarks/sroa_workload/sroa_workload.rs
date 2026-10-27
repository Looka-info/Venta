struct Vec3 {
    x: i64,
    y: i64,
    z: i64,
}

#[inline(always)]
fn process(v: &Vec3) -> i64 {
    v.x + v.y + v.z
}

fn compute_sroa(iters: i64) -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < iters {
        let v = Vec3 {
            x: i,
            y: i + 1,
            z: i + 2,
        };
        sum += process(&v);
        i += 1;
    }
    sum
}

fn main() {
    let res = compute_sroa(20000000);
    println!("{}", res);
}
