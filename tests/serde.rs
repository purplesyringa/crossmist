use crossmist::{Deserializer, FnOnceObject, Object, Serializer, lambda};
use std::fmt::Debug;

mod testing;
testing::setup!();

fn serde<T: Object>(x: T) -> T {
    let mut s = Serializer::new();
    s.serialize(x);
    let mut d = Deserializer::from(s);
    unsafe { d.deserialize() }
}

fn test_idempotency<T: Object + Clone + PartialEq + Debug>(x: T) {
    assert_eq!(serde(x.clone()), x);
}

#[derive(Clone, Debug, PartialEq, Object)]
struct SimplePair {
    x: i32,
    y: i32,
}

#[macro_rules_attribute::apply(test!)]
fn simple() {
    test_idempotency(0x123456789abcdefi64);
}

#[macro_rules_attribute::apply(test!)]
fn string() {
    test_idempotency("hello".to_string());
}

#[macro_rules_attribute::apply(test!)]
fn complex_argument() {
    test_idempotency(SimplePair { x: 5, y: 7 })
}

#[macro_rules_attribute::apply(test!)]
fn hole() {
    test_idempotency((1i32, 2u8, 3i32))
}

#[macro_rules_attribute::apply(test!)]
fn boxed() {
    test_idempotency(Box::new(7))
}

#[macro_rules_attribute::apply(test!)]
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
        format!("bool says: {self}")
    }
}

#[macro_rules_attribute::apply(test!)]
fn box_trait() {
    assert_eq!(
        serde(Box::new(ImplA("hello".to_string())) as Box<dyn Trait>).say(),
        "ImplA says: hello"
    );
    assert_eq!(
        serde(Box::new(ImplB(5)) as Box<dyn Trait>).say(),
        "ImplB says: 5"
    );
    assert_eq!(
        serde(Box::new(true) as Box<dyn Trait>).say(),
        "bool says: true"
    );
}

#[macro_rules_attribute::apply(test!)]
fn function() {
    let func: Box<dyn FnOnceObject<(i32, i32), Output = i32>> = lambda! { |a, b| a + b };
    assert_eq!(serde(func).call_object_box((5, 7)), 12);
}

#[macro_rules_attribute::apply(test!)]
fn bound_function() {
    let a = 5;
    let func: Box<dyn FnOnceObject<(i32,), Output = i32>> = lambda! { move(a: i32) |b| a + b };
    assert_eq!(serde(func).call_object_box((7,)), 12);
}

#[macro_rules_attribute::apply(test!)]
fn ref_bound_function() {
    let s = "abc".to_string();
    let func: Box<dyn FnOnceObject<(), Output = usize>> =
        lambda! { move(ref s: String) || s.len() };
    assert_eq!(serde(func).call_object_box(()), 3);
}

#[macro_rules_attribute::apply(test!)]
fn double_bound_function() {
    let a = 5;
    let b = 7;
    let func: Box<dyn FnOnceObject<(), Output = i32>> = lambda! { move(a: i32, b: i32) || a + b };
    assert_eq!(serde(func).call_object_box(()), 12);
}

#[macro_rules_attribute::apply(test!)]
#[cfg(not(miri))]
fn test_rx() {
    let (mut tx, rx) = crossmist::channel::<i32>().unwrap();
    let mut rx = serde(rx);
    tx.send(5).unwrap();
    tx.send(7).unwrap();
    assert_eq!(rx.recv().unwrap().unwrap(), 5);
    assert_eq!(rx.recv().unwrap().unwrap(), 7);
}

#[macro_rules_attribute::apply(test!)]
#[cfg(not(miri))]
fn test_tx() {
    let (tx, mut rx) = crossmist::channel::<i32>().unwrap();
    let mut tx = serde(tx);
    tx.send(5).unwrap();
    tx.send(7).unwrap();
    assert_eq!(rx.recv().unwrap().unwrap(), 5);
    assert_eq!(rx.recv().unwrap().unwrap(), 7);
}

#[macro_rules_attribute::apply(test!)]
#[cfg(not(miri))]
fn test_duplex() {
    let (mut local, downstream) = crossmist::duplex::<(i32, i32), i32>().unwrap();
    let mut downstream = serde(downstream);
    for (x, y) in [(5, 7), (100, -1), (53, 2354)] {
        local.send((x, y)).unwrap();
        let (x1, y1) = downstream.recv().unwrap().unwrap();
        downstream.send(x1 - y1).unwrap();
        assert_eq!(local.recv().unwrap().unwrap(), x1 - y1);
    }
    drop(local);
    assert!(downstream.recv().unwrap().is_none());
}
