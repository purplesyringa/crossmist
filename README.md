# crossmist

![License: MIT](https://img.shields.io/crates/l/crossmist)
[![docs.rs](https://img.shields.io/docsrs/crossmist)](https://docs.rs/crossmist/latest/crossmist/)
[![crates.io](https://img.shields.io/crates/v/crossmist)](https://crates.io/crates/crossmist)

crossmist provides efficient and seamless cross-process communication for Rust. It provides semantics similar to `std::thread::spawn` and single-producer single-consumer channels, both synchronously and asynchronously.


## Installation

```shell
$ cargo add crossmist
```

Or add the following to your `Cargo.toml`:

```toml
crossmist = "1.1"
```


## Documentation

Check out [docs.rs](https://docs.rs/crossmist/latest/crossmist/).


## Motivational examples

This crate allows you to easily perform computations in another process without creating a separate executable or parsing command line arguments manually. For example, the simplest example, computing a sum of several numbers in a one-shot subprocess, looks like this:

```rust
#[crossmist::main]
fn main() {
    println!("5 + 7 = {}", add.run(vec![5, 7]).unwrap());
}

#[crossmist::func]
fn add(nums: Vec<i32>) -> i32 {
    nums.into_iter().sum()
}
```

This crate also supports long-lived tasks with constant cross-process communication:

```rust
#[crossmist::main]
fn main() {
    let (mut ours, theirs) = crossmist::duplex().unwrap();
    add.spawn(theirs).expect("Failed to spawn child");
    for i in 1..=5 {
        for j in 1..=5 {
            println!("{i} + {j} = {}", ours.request(&vec![i, j]).unwrap());
        }
    }
}

#[crossmist::func]
fn add(mut chan: crossmist::Duplex<i32, Vec<i32>>) {
    while let Some(nums) = chan.recv().unwrap() {
        chan.send(&nums.into_iter().sum());
    }
}
```

Almost arbitrary objects can be passed between processes and across channels, including file handles, sockets, and other channels.
