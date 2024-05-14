use crossmist::tokio::{channel, duplex, Duplex, Receiver, Sender};
use crossmist::{FnOnceObject, Object};

#[ctor::ctor]
fn ctor() {
    crossmist::init();
}

#[derive(Debug, PartialEq, Object)]
struct SimplePair {
    x: i32,
    y: i32,
}

#[tokio::test(flavor = "current_thread")]
async fn simple() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner() -> i64 {
        0x123456789abcdef
    }
    assert_eq!(inner.run_tokio().await.unwrap(), 0x123456789abcdef);
    assert_eq!(
        inner.spawn_tokio().await.unwrap().join().await.unwrap(),
        0x123456789abcdef
    );
}

#[tokio::test(flavor = "current_thread")]
async fn add_with_arguments() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(x: i32, y: i32) -> i32 {
        x + y
    }
    assert_eq!(inner.call_object_once((5, 7)).await, 12);
    assert_eq!(
        inner.spawn_tokio(5, 7).await.unwrap().join().await.unwrap(),
        12
    );
}

#[tokio::test(flavor = "current_thread")]
async fn swap_complex_argument() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(pair: SimplePair) -> SimplePair {
        SimplePair {
            x: pair.y,
            y: pair.x,
        }
    }
    assert_eq!(
        inner
            .spawn_tokio(SimplePair { x: 5, y: 7 })
            .await
            .unwrap()
            .join()
            .await
            .unwrap(),
        SimplePair { x: 7, y: 5 }
    );
}

#[tokio::test(flavor = "current_thread")]
async fn with_passed_rx() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(mut rx: Receiver<i32>) -> i32 {
        let a = rx.recv().await.unwrap().unwrap();
        let b = rx.recv().await.unwrap().unwrap();
        a - b
    }
    let (mut tx, rx) = channel::<i32>().unwrap();
    let child = inner.spawn_tokio(rx).await.unwrap();
    tx.send(&5).await.unwrap();
    tx.send(&7).await.unwrap();
    assert_eq!(child.join().await.unwrap(), -2);
}

#[tokio::test(flavor = "current_thread")]
async fn with_passed_tx() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(mut tx: Sender<i32>) {
        tx.send(&5).await.unwrap();
        tx.send(&7).await.unwrap();
    }
    let (tx, mut rx) = channel::<i32>().unwrap();
    let child = inner.spawn_tokio(tx).await.unwrap();
    assert_eq!(
        rx.recv().await.unwrap().unwrap() - rx.recv().await.unwrap().unwrap(),
        -2
    );
    child.join().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn with_passed_duplex() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(mut chan: Duplex<i32, (i32, i32)>) {
        while let Some((x, y)) = chan.recv().await.unwrap() {
            chan.send(&(x - y)).await.unwrap();
        }
    }
    let (mut local, downstream) = duplex::<(i32, i32), i32>().unwrap();
    let child = inner.spawn_tokio(downstream).await.unwrap();
    for (x, y) in [(5, 7), (100, -1), (53, 2354)] {
        local.send(&(x, y)).await.unwrap();
        assert_eq!(local.recv().await.unwrap().unwrap(), x - y);
    }
    drop(local);
    child.join().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn with_passed_nested_channel() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(mut chan: Receiver<Receiver<i32>>) -> i32 {
        let mut chan1 = chan.recv().await.unwrap().unwrap();
        chan1.recv().await.unwrap().unwrap()
    }
    let (mut tx, rx) = channel::<i32>().unwrap();
    let (mut tx1, rx1) = channel::<Receiver<i32>>().unwrap();
    tx.send(&5).await.unwrap();
    tx1.send(&rx).await.unwrap();
    assert_eq!(inner.run_tokio(rx1).await.unwrap(), 5);
}

#[tokio::test(flavor = "current_thread")]
async fn with_async_write() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner(mut tx_data: Sender<i32>, mut tx_signal: Sender<()>) {
        let future = tokio::spawn(async move {
            for i in 0..1000 {
                tx_data.send(&i).await.unwrap();
            }
        });
        tx_signal.send(&()).await.unwrap();
        future.await.unwrap();
    }
    let (tx_data, mut rx_data) = channel().unwrap();
    let (tx_signal, mut rx_signal) = channel().unwrap();
    let child = inner.spawn_tokio(tx_data, tx_signal).await.unwrap();
    rx_signal.recv().await.unwrap();
    for i in 0..1000 {
        assert_eq!(rx_data.recv().await.unwrap().unwrap(), i);
    }
    child.join().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn exitting() {
    #[crossmist::func(tokio(flavor = "current_thread"))]
    async fn inner() {
        std::process::exit(0);
    }
    assert_eq!(inner.run_tokio().await.unwrap(), ());
}
