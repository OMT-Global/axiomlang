use std::sync::{mpsc, Mutex, OnceLock};

fn main() {
    let counter = Mutex::new(1);
    *counter.lock().unwrap() = 2;
    println!("{}", *counter.lock().unwrap());

    let ready = OnceLock::new();
    ready.set("configured").unwrap();
    println!("{}", ready.get().is_some());

    let empty: OnceLock<i32> = OnceLock::new();
    match empty.get() {
        Some(value) => println!("{value}"),
        None => println!("empty"),
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    sender.send("message").unwrap();
    match receiver.try_recv() {
        Ok(message) => println!("{message}"),
        Err(_) => println!("missing"),
    }
}
