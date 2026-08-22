use crossmist::{Deserializer, Object, Serializer};
use std::fmt::Debug;

fn test_idempotency<T: Object + Clone + PartialEq + Debug>(x: T) {
    let mut s = Serializer::new();
    s.serialize(x.clone());
    let mut d = Deserializer::from(s);
    assert_eq!(unsafe { d.deserialize::<T>() }, x);
}

fn test_empty<T: Object>(x: T) {
    let mut s = Serializer::new();
    s.serialize(x);
    s.serialize(12345i32);
    let mut d = Deserializer::from(s);
    assert!(unsafe { d.deserialize::<i32>() } == 12345);
}

#[derive(Clone, Debug, Object, PartialEq)]
struct NamedStruct {
    x: i32,
    y: i32,
}

#[derive(Clone, Debug, Object, PartialEq)]
struct UnnamedStruct(i32, i32);

#[derive(Clone, Debug, Object, PartialEq)]
struct UnitStruct;

#[derive(Clone, Debug, Object, PartialEq)]
struct EmptyStruct {}

#[derive(Clone, Debug, Object, PartialEq)]
enum NamedEnum {
    Variant1 { x: i32, y: i32 },
    Variant2 { x: i32, y: i32 },
}

#[derive(Clone, Debug, Object, PartialEq)]
enum UnnamedEnum {
    Variant1(i32, i32),
    Variant2(i32, i32),
}

#[derive(Clone, Debug, Object, PartialEq)]
enum UnitEnum {
    Variant1,
    Variant2,
}

#[derive(Clone, Debug, Object, PartialEq)]
enum EmptyEnum {
    Variant1 {},
    Variant2 {},
}

#[derive(Clone, Debug, Object, PartialEq)]
enum SingleVariantEnum {
    Variant,
}

#[derive(Clone, Debug, Object, PartialEq)]
#[allow(dead_code)]
enum NoVariantEnum {}

#[test]
fn test() {
    test_idempotency(NamedStruct { x: 100, y: 200 });
    test_idempotency(UnnamedStruct(100, 200));
    test_idempotency(UnitStruct);
    test_idempotency(EmptyStruct {});
    test_idempotency(NamedEnum::Variant1 { x: 100, y: 200 });
    test_idempotency(NamedEnum::Variant2 { x: 100, y: 200 });
    test_idempotency(UnnamedEnum::Variant1(100, 200));
    test_idempotency(UnnamedEnum::Variant2(100, 200));
    test_idempotency(UnitEnum::Variant1);
    test_idempotency(UnitEnum::Variant2);
    test_idempotency(EmptyEnum::Variant1 {});
    test_idempotency(EmptyEnum::Variant2 {});
    test_idempotency(SingleVariantEnum::Variant);

    test_empty(UnitStruct);
    test_empty(EmptyStruct {});
    test_empty(SingleVariantEnum::Variant);
}
