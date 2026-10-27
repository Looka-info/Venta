struct Point {
    x: i64,
    y: i64,
    z: i64,
}

#[inline(never)]
fn manhattan(p: &Point) -> i64 {
    let mut x = p.x;
    if x < 0 {
        x = -x;
    }
    let mut y = p.y;
    if y < 0 {
        y = -y;
    }
    let mut z = p.z;
    if z < 0 {
        z = -z;
    }
    x + y + z
}

fn main() {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < 1000000 {
        let p = Point {
            x: i % 100 - 50,
            y: (i * 3) % 100 - 50,
            z: (i * 7) % 100 - 50,
        };
        total += manhattan(&p);
        i += 1;
    }
    println!("{}", total);
}
