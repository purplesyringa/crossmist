use crossmist::{lambda, Deserializer, FnOnceObject, Object, Serializer};
use std::fmt::Debug;

fn serde<T: Object>(x: &T) -> T {
    let mut s = Serializer::new();
    s.serialize(x);
    let handles = s
        .drain_handles()
        .into_iter()
        .map(|handle| handle.try_clone_to_owned().unwrap())
        .collect();
    let data = s.into_vec();
    let mut d = Deserializer::new(data, handles);
    unsafe { d.deserialize() }.expect("Deserialization failed")
}

fn test_idempotency<T: Object + PartialEq + Debug>(x: T) {
    assert_eq!(serde(&x), x);
}

#[derive(Debug, PartialEq, Object)]
struct SimplePair {
    x: i32,
    y: i32,
}

#[test]
fn simple() {
    test_idempotency(0x123456789abcdefi64);
}

#[test]
fn string() {
    test_idempotency("hello".to_string());
}

#[test]
fn complex_argument() {
    test_idempotency(SimplePair { x: 5, y: 7 })
}

#[test]
fn hole() {
    test_idempotency((1i32, 2u8, 3i32))
}

#[test]
fn boxed() {
    test_idempotency(Box::new(7))
}

#[test]
fn vec_and_box() {
    test_idempotency((vec![1, 2, 3], Box::new([4, 5, 6])))
}

trait Trait: Object {
    fn say(&self) -> String;
}

#[derive(Object)]
struct ImplA(String);

#[derive(Object)]
struct ImplB(i32);

impl Trait for ImplA {
    fn say(&self) -> String {
        format!("ImplA says: {}", self.0)
    }
}

impl Trait for ImplB {
    fn say(&self) -> String {
        format!("ImplB says: {}", self.0)
    }
}

impl Trait for bool {
    fn say(&self) -> String {
        format!("bool says: {}", self)
    }
}

#[test]
fn box_trait() {
    assert_eq!(
        serde(&(Box::new(ImplA("hello".to_string())) as Box<dyn Trait>)).say(),
        "ImplA says: hello"
    );
    assert_eq!(
        serde(&(Box::new(ImplB(5)) as Box<dyn Trait>)).say(),
        "ImplB says: 5"
    );
    assert_eq!(
        serde(&(Box::new(true) as Box<dyn Trait>)).say(),
        "bool says: true"
    );
}

#[test]
fn function() {
    let func: Box<dyn FnOnceObject<(i32, i32), Output = i32>> =
        lambda! { |a: i32, b: i32| -> i32 { a + b } };
    assert_eq!(serde(&func).call_object_box((5, 7)), 12);
}

#[test]
fn bound_function() {
    let a = 5;
    let func: Box<dyn FnOnceObject<(i32,), Output = i32>> =
        lambda! { move(a: i32) |b: i32| -> i32 { a + b } };
    assert_eq!(serde(&func).call_object_box((7,)), 12);
}

#[test]
fn double_bound_function() {
    let a = 5;
    let b = 7;
    let func: Box<dyn FnOnceObject<(), Output = i32>> =
        lambda! { move(a: i32, b: i32) || -> i32 { a + b } };
    assert_eq!(serde(&func).call_object_box(()), 12);
}

#[test]
#[cfg(not(miri))]
fn test_rx() {
    let (mut tx, rx) = crossmist::channel::<i32>().unwrap();
    let mut rx = serde(&rx);
    tx.send(&5).unwrap();
    tx.send(&7).unwrap();
    assert_eq!(rx.recv().unwrap().unwrap(), 5);
    assert_eq!(rx.recv().unwrap().unwrap(), 7);
}

#[test]
#[cfg(not(miri))]
fn test_tx() {
    let (tx, mut rx) = crossmist::channel::<i32>().unwrap();
    let mut tx = serde(&tx);
    tx.send(&5).unwrap();
    tx.send(&7).unwrap();
    assert_eq!(rx.recv().unwrap().unwrap(), 5);
    assert_eq!(rx.recv().unwrap().unwrap(), 7);
}

#[test]
#[cfg(not(miri))]
fn test_duplex() {
    let (mut local, downstream) = crossmist::duplex::<(i32, i32), i32>().unwrap();
    let mut downstream = serde(&downstream);
    for (x, y) in [(5, 7), (100, -1), (53, 2354)] {
        local.send(&(x, y)).unwrap();
        let (x1, y1) = downstream.recv().unwrap().unwrap();
        downstream.send(&(x1 - y1)).unwrap();
        assert_eq!(local.recv().unwrap().unwrap(), x1 - y1);
    }
    drop(local);
    assert!(downstream.recv().unwrap().is_none());
}
