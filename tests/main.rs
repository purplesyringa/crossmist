use crossmist::{
    channel, duplex, static_ref, BindValue, Duplex, FnOnceObject, Object, Receiver, Sender,
    StaticRef,
};

#[ctor::ctor]
fn ctor() {
    crossmist::init();
}

#[derive(Debug, PartialEq, Object)]
struct SimplePair {
    x: i32,
    y: i32,
}

#[test]
fn simple() {
    #[crossmist::func]
    fn inner() -> i64 {
        0x123456789abcdef
    }
    assert_eq!(inner.run().unwrap(), 0x123456789abcdef);
}

#[test]
fn ret_string() {
    #[crossmist::func]
    fn inner() -> String {
        "hello".to_string()
    }
    assert_eq!(inner.spawn().unwrap().join().unwrap(), "hello");
}

#[crossmist::func]
fn add_with_arguments_impl(x: i32, y: i32) -> i32 {
    x + y
}

#[test]
fn add_with_arguments_spawn() {
    assert_eq!(
        add_with_arguments_impl.spawn(5, 7).unwrap().join().unwrap(),
        12
    );
}

#[test]
fn add_with_arguments_call() {
    assert_eq!(add_with_arguments_impl.call_object_once((5, 7)), 12);
    #[cfg(feature = "nightly")]
    assert_eq!(add_with_arguments_impl(5, 7), 12);
}

#[test]
fn add_with_template() {
    #[crossmist::func]
    fn inner<T: std::ops::Add<Output = T> + Object + 'static>(x: T, y: T) -> T {
        x + y
    }
    assert_eq!(inner.spawn(5, 7).unwrap().join().unwrap(), 12);
}

#[test]
fn swap_complex_argument() {
    #[crossmist::func]
    fn inner(pair: SimplePair) -> SimplePair {
        SimplePair {
            x: pair.y,
            y: pair.x,
        }
    }
    assert_eq!(
        inner
            .spawn(SimplePair { x: 5, y: 7 })
            .unwrap()
            .join()
            .unwrap(),
        SimplePair { x: 7, y: 5 }
    );
}

#[test]
fn inc_with_boxed() {
    #[crossmist::func]
    fn inner(item: Box<i32>) -> Box<i32> {
        Box::new(*item + 1)
    }
    assert_eq!(*inner.spawn(Box::new(7)).unwrap().join().unwrap(), 8);
}

#[test]
fn inc_with_vec_and_box() {
    #[crossmist::func]
    fn inner(vec: Vec<i32>, box_: Box<[i32]>) -> (i32, i32) {
        (vec.iter().sum(), box_.iter().sum())
    }
    assert_eq!(
        inner
            .spawn(vec![1, 2, 3], Box::new([4, 5, 6]))
            .unwrap()
            .join()
            .unwrap(),
        (6, 15)
    );
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
fn with_passed_trait() {
    #[crossmist::func]
    fn inner(arg: Box<dyn Trait>) -> String {
        arg.say()
    }
    assert_eq!(
        inner
            .spawn(Box::new(ImplA("hello".to_string())))
            .unwrap()
            .join()
            .unwrap(),
        "ImplA says: hello"
    );
    assert_eq!(
        inner.spawn(Box::new(ImplB(5))).unwrap().join().unwrap(),
        "ImplB says: 5"
    );
    assert_eq!(
        inner.spawn(Box::new(true)).unwrap().join().unwrap(),
        "bool says: true"
    );
}

#[test]
fn with_passed_fn() {
    #[crossmist::func]
    fn inner(func: Box<dyn crossmist::FnOnceObject<(i32, i32), Output = i32>>) -> i32 {
        #[cfg(feature = "nightly")]
        {
            func(5, 7)
        }
        #[cfg(not(feature = "nightly"))]
        {
            func.call_object_once((5, 7))
        }
    }
    assert_eq!(
        inner
            .spawn(Box::new(add_with_arguments_impl))
            .unwrap()
            .join()
            .unwrap(),
        12
    );
}

#[test]
fn with_passed_bound_fn() {
    #[crossmist::func]
    fn inner(func: Box<dyn crossmist::FnOnceObject<(i32,), Output = i32>>) -> i32 {
        #[cfg(feature = "nightly")]
        {
            func(7)
        }
        #[cfg(not(feature = "nightly"))]
        {
            func.call_object_once((7,))
        }
    }
    assert_eq!(
        inner
            .spawn(Box::new(add_with_arguments_impl.bind_value(5)))
            .unwrap()
            .join()
            .unwrap(),
        12
    );
}

#[test]
fn with_passed_double_bound_fn() {
    #[crossmist::func]
    fn inner(func: Box<dyn crossmist::FnOnceObject<(), Output = i32>>) -> i32 {
        #[cfg(feature = "nightly")]
        {
            func()
        }
        #[cfg(not(feature = "nightly"))]
        {
            func.call_object_once(())
        }
    }
    assert_eq!(
        inner
            .spawn(Box::new(
                add_with_arguments_impl.bind_value(5).bind_value(7)
            ))
            .unwrap()
            .join()
            .unwrap(),
        12
    );
}

#[test]
fn with_passed_rx() {
    #[crossmist::func]
    fn inner(mut rx: Receiver<i32>) -> i32 {
        let a = rx.recv().unwrap().unwrap();
        let b = rx.recv().unwrap().unwrap();
        a - b
    }
    let (mut tx, rx) = channel::<i32>().unwrap();
    let child = inner.spawn(rx).unwrap();
    tx.send(&5).unwrap();
    tx.send(&7).unwrap();
    assert_eq!(child.join().unwrap(), -2);
}

#[test]
fn with_passed_tx() {
    #[crossmist::func]
    fn inner(mut tx: Sender<i32>) {
        tx.send(&5).unwrap();
        tx.send(&7).unwrap();
    }
    let (tx, mut rx) = channel::<i32>().unwrap();
    let child = inner.spawn(tx).unwrap();
    assert_eq!(
        rx.recv().unwrap().unwrap() - rx.recv().unwrap().unwrap(),
        -2
    );
    child.join().unwrap();
}

#[test]
fn with_passed_duplex() {
    #[crossmist::func]
    fn inner(mut chan: Duplex<i32, (i32, i32)>) {
        while let Some((x, y)) = chan.recv().unwrap() {
            chan.send(&(x - y)).unwrap();
        }
    }
    let (mut local, downstream) = duplex::<(i32, i32), i32>().unwrap();
    let child = inner.spawn(downstream).unwrap();
    for (x, y) in [(5, 7), (100, -1), (53, 2354)] {
        local.send(&(x, y)).unwrap();
        assert_eq!(local.recv().unwrap().unwrap(), x - y);
    }
    drop(local);
    child.join().unwrap();
}

#[test]
fn with_passed_nested_channel() {
    #[crossmist::func]
    fn inner(mut chan: Receiver<Receiver<i32>>) -> i32 {
        let mut chan1 = chan.recv().unwrap().unwrap();
        chan1.recv().unwrap().unwrap()
    }
    let (mut tx, rx) = channel::<i32>().unwrap();
    let (mut tx1, rx1) = channel::<Receiver<i32>>().unwrap();
    tx.send(&5).unwrap();
    tx1.send(&rx).unwrap();
    assert_eq!(inner.run(rx1).unwrap(), 5);
}

#[test]
fn exitting() {
    #[crossmist::func]
    fn inner() {
        std::process::exit(0);
    }
    assert_eq!(inner.run().unwrap(), ());
}

#[test]
fn with_static_ref() {
    #[crossmist::func]
    fn inner(a: StaticRef<&'static str>) -> String {
        a.to_string()
    }
    assert_eq!(
        inner
            .run(static_ref!(&'static str, &"Hello, world!"))
            .unwrap(),
        "Hello, world!"
    );
}
