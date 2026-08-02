use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() {
    let start = Instant::now();
    let wall_clock_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    println!("{}", wall_clock_ms > 0);
    println!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            > 0
    );
    std::thread::sleep(Duration::from_millis(0));
    println!("true");
    let elapsed = start.elapsed().as_millis();
    println!("{}", elapsed == elapsed);
}
