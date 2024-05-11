use crossmist::{
    channel, duplex, static_ref, BindValue, Duplex, Object, Receiver, Sender, StaticRef,
};

#[derive(Debug, PartialEq, Object)]
struct SimplePair {
    x: i32,
    y: i32,
}

#[crossmist::func]
fn simple() -> i64 {
    0x123456789abcdef
}

#[crossmist::func]
fn ret_string() -> String {
    "hello".to_string()
}

#[crossmist::func]
fn add_with_arguments(x: i32, y: i32) -> i32 {
    x + y
}

#[crossmist::func]
fn add_with_template<T: std::ops::Add<Output = T> + Object + 'static>(x: T, y: T) -> T {
    x + y
}

#[crossmist::func]
fn swap_complex_argument(pair: SimplePair) -> SimplePair {
    SimplePair {
        x: pair.y,
        y: pair.x,
    }
}

#[crossmist::func]
fn inc_with_boxed(item: Box<i32>) -> Box<i32> {
    Box::new(*item + 1)
}

#[crossmist::func]
fn inc_with_vec_and_box(vec: Vec<i32>, box_: Box<[i32]>) -> (i32, i32) {
    (vec.iter().sum(), box_.iter().sum())
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

#[crossmist::func]
fn with_passed_trait(arg: Box<dyn Trait>) -> String {
    arg.say()
}

#[crossmist::func]
fn with_passed_fn(func: Box<dyn crossmist::FnOnceObject<(i32, i32), Output = i32>>) -> i32 {
    func(5, 7)
}

#[crossmist::func]
fn with_passed_bound_fn(func: Box<dyn crossmist::FnOnceObject<(i32,), Output = i32>>) -> i32 {
    func(7)
}

#[crossmist::func]
fn with_passed_double_bound_fn(func: Box<dyn crossmist::FnOnceObject<(), Output = i32>>) -> i32 {
    func()
}

#[crossmist::func]
fn with_passed_rx(mut rx: Receiver<i32>) -> i32 {
    let a = rx.recv().unwrap().unwrap();
    let b = rx.recv().unwrap().unwrap();
    a - b
}

#[crossmist::func]
fn with_passed_tx(mut tx: Sender<i32>) {
    tx.send(&5).unwrap();
    tx.send(&7).unwrap();
}

#[crossmist::func]
fn with_passed_duplex(mut chan: Duplex<i32, (i32, i32)>) {
    while let Some((x, y)) = chan.recv().unwrap() {
        chan.send(&(x - y)).unwrap();
    }
}

#[crossmist::func]
fn with_passed_nested_channel(mut chan: Receiver<Receiver<i32>>) -> i32 {
    let mut chan1 = chan.recv().unwrap().unwrap();
    chan1.recv().unwrap().unwrap()
}

#[crossmist::func]
fn exitting() {
    std::process::exit(0);
}

#[crossmist::func]
fn with_static_ref(a: StaticRef<&'static str>) -> String {
    a.to_string()
}

#[crossmist::main]
fn main() {
    assert_eq!(simple.run().expect("simple run failed"), 0x123456789abcdef);
    println!("simple run OK");

    assert_eq!(
        simple.spawn().unwrap().join().expect("simple failed"),
        0x123456789abcdef
    );
    println!("simple OK");

    assert_eq!(
        ret_string
            .spawn()
            .unwrap()
            .join()
            .expect("ret_string failed"),
        "hello"
    );
    println!("ret_string OK");

    assert_eq!(
        add_with_arguments
            .spawn(5, 7)
            .unwrap()
            .join()
            .expect("add_with_arguments failed"),
        12
    );
    println!("add_with_arguments OK");

    assert_eq!(add_with_arguments(5, 7), 12);
    println!("add_with_arguments call OK");

    assert_eq!(
        add_with_template
            .spawn(5, 7)
            .unwrap()
            .join()
            .expect("add_with_template failed"),
        12
    );
    println!("add_with_template OK");

    assert_eq!(
        swap_complex_argument
            .spawn(SimplePair { x: 5, y: 7 })
            .unwrap()
            .join()
            .expect("swap_complex_argument failed"),
        SimplePair { x: 7, y: 5 }
    );
    println!("swap_complex_argument OK");

    assert_eq!(
        *inc_with_boxed
            .spawn(Box::new(7))
            .unwrap()
            .join()
            .expect("inc_with_boxed failed"),
        8
    );
    println!("inc_with_boxed OK");

    assert_eq!(
        inc_with_vec_and_box
            .spawn(vec![1, 2, 3], Box::new([4, 5, 6]))
            .unwrap()
            .join()
            .expect("inc_with_vec_and_box failed"),
        (6, 15)
    );
    println!("inc_with_vec_and_box OK");

    assert_eq!(
        with_passed_trait
            .spawn(Box::new(ImplA("hello".to_string())))
            .unwrap()
            .join()
            .expect("with_passed_trait failed"),
        "ImplA says: hello"
    );
    assert_eq!(
        with_passed_trait
            .spawn(Box::new(ImplB(5)))
            .unwrap()
            .join()
            .expect("with_passed_trait failed"),
        "ImplB says: 5"
    );
    assert_eq!(
        with_passed_trait
            .spawn(Box::new(true))
            .unwrap()
            .join()
            .expect("with_passed_trait failed"),
        "bool says: true"
    );
    println!("with_passed_trait OK");

    assert_eq!(
        with_passed_fn
            .spawn(Box::new(add_with_arguments))
            .unwrap()
            .join()
            .expect("with_passed_fn failed"),
        12
    );
    println!("with_passed_fn OK");

    assert_eq!(
        with_passed_bound_fn
            .spawn(Box::new(add_with_arguments.bind_value(5)))
            .unwrap()
            .join()
            .expect("with_passed_bound_fn failed"),
        12
    );
    println!("with_passed_bound_fn OK");

    assert_eq!(
        with_passed_double_bound_fn
            .spawn(Box::new(add_with_arguments.bind_value(5).bind_value(7)))
            .unwrap()
            .join()
            .expect("with_passed_double_bound_fn failed"),
        12
    );
    println!("with_passed_double_bound_fn OK");

    {
        let (mut tx, rx) = channel::<i32>().unwrap();
        let child = with_passed_rx.spawn(rx).unwrap();
        tx.send(&5).unwrap();
        tx.send(&7).unwrap();
        assert_eq!(child.join().expect("with_passed_rx failed"), -2);
        println!("with_passed_rx OK");
    }

    {
        let (tx, mut rx) = channel::<i32>().unwrap();
        let child = with_passed_tx.spawn(tx).unwrap();
        assert_eq!(
            rx.recv().unwrap().unwrap() - rx.recv().unwrap().unwrap(),
            -2
        );
        child.join().unwrap();
        println!("with_passed_tx OK");
    }

    {
        let (mut local, downstream) = duplex::<(i32, i32), i32>().unwrap();
        let child = with_passed_duplex.spawn(downstream).unwrap();
        for (x, y) in [(5, 7), (100, -1), (53, 2354)] {
            local.send(&(x, y)).unwrap();
            assert_eq!(local.recv().unwrap().unwrap(), x - y);
        }
        drop(local);
        child.join().unwrap();
        println!("with_passed_duplex OK");
    }

    {
        let (mut tx, rx) = channel::<i32>().unwrap();
        let (mut tx1, rx1) = channel::<Receiver<i32>>().unwrap();
        tx.send(&5).unwrap();
        tx1.send(&rx).unwrap();
        assert_eq!(with_passed_nested_channel.run(rx1).unwrap(), 5);
        println!("with_passed_nested_channel OK");
    }

    {
        assert_eq!(exitting.run().unwrap(), ());
        println!("exitting OK");
    }

    {
        assert_eq!(
            with_static_ref
                .run(static_ref!(&'static str, &"Hello, world!"))
                .unwrap(),
            "Hello, world!"
        );
        println!("with_static_ref OK");
    }
}
