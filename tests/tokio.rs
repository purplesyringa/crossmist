use crossmist::tokio::{channel, duplex, Duplex, Receiver, Sender};
use crossmist::Object;

#[derive(Debug, PartialEq, Object)]
struct SimplePair {
    x: i32,
    y: i32,
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn simple() -> i64 {
    0x123456789abcdef
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn add_with_arguments(x: i32, y: i32) -> i32 {
    x + y
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn swap_complex_argument(pair: SimplePair) -> SimplePair {
    SimplePair {
        x: pair.y,
        y: pair.x,
    }
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn with_passed_rx(mut rx: Receiver<i32>) -> i32 {
    let a = rx.recv().await.unwrap().unwrap();
    let b = rx.recv().await.unwrap().unwrap();
    a - b
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn with_passed_tx(mut tx: Sender<i32>) {
    tx.send(&5).await.unwrap();
    tx.send(&7).await.unwrap();
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn with_passed_duplex(mut chan: Duplex<i32, (i32, i32)>) {
    while let Some((x, y)) = chan.recv().await.unwrap() {
        chan.send(&(x - y)).await.unwrap();
    }
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn with_passed_nested_channel(mut chan: Receiver<Receiver<i32>>) -> i32 {
    let mut chan1 = chan.recv().await.unwrap().unwrap();
    chan1.recv().await.unwrap().unwrap()
}

#[crossmist::func]
#[tokio::main(flavor = "current_thread")]
async fn with_async_write(mut tx_data: Sender<i32>, mut tx_signal: Sender<()>) {
    let future = tokio::spawn(async move {
        for i in 0..1000 {
            tx_data.send(&i).await.unwrap();
        }
    });
    tx_signal.send(&()).await.unwrap();
    future.await.unwrap();
}

#[crossmist::main]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    assert_eq!(
        simple.run_tokio().await.expect("simple run failed"),
        0x123456789abcdef
    );
    println!("simple run OK");

    assert_eq!(
        simple
            .spawn_tokio()
            .await
            .unwrap()
            .join()
            .await
            .expect("simple failed"),
        0x123456789abcdef
    );
    println!("simple OK");

    assert_eq!(
        add_with_arguments
            .spawn_tokio(5, 7)
            .await
            .unwrap()
            .join()
            .await
            .expect("add_with_arguments failed"),
        12
    );
    println!("add_with_arguments OK");

    assert_eq!(add_with_arguments(5, 7).await, 12);
    println!("add_with_arguments call OK");

    assert_eq!(
        swap_complex_argument
            .spawn_tokio(SimplePair { x: 5, y: 7 })
            .await
            .unwrap()
            .join()
            .await
            .expect("swap_complex_argument failed"),
        SimplePair { x: 7, y: 5 }
    );
    println!("swap_complex_argument OK");

    {
        let (mut tx, rx) = channel::<i32>().unwrap();
        let child = with_passed_rx.spawn_tokio(rx).await.unwrap();
        tx.send(&5).await.unwrap();
        tx.send(&7).await.unwrap();
        assert_eq!(child.join().await.expect("with_passed_rx failed"), -2);
        println!("with_passed_rx OK");
    }

    {
        let (tx, mut rx) = channel::<i32>().unwrap();
        let child = with_passed_tx.spawn_tokio(tx).await.unwrap();
        assert_eq!(
            rx.recv().await.unwrap().unwrap() - rx.recv().await.unwrap().unwrap(),
            -2
        );
        child.join().await.unwrap();
        println!("with_passed_tx OK");
    }

    {
        let (mut local, downstream) = duplex::<(i32, i32), i32>().unwrap();
        let child = with_passed_duplex.spawn_tokio(downstream).await.unwrap();
        for (x, y) in [(5, 7), (100, -1), (53, 2354)] {
            local.send(&(x, y)).await.unwrap();
            assert_eq!(local.recv().await.unwrap().unwrap(), x - y);
        }
        drop(local);
        child.join().await.unwrap();
        println!("with_passed_duplex OK");
    }

    {
        let (mut tx, rx) = channel::<i32>().unwrap();
        let (mut tx1, rx1) = channel::<Receiver<i32>>().unwrap();
        tx.send(&5).await.unwrap();
        tx1.send(&rx).await.unwrap();
        assert_eq!(with_passed_nested_channel.run_tokio(rx1).await.unwrap(), 5);
        println!("with_passed_nested_channel OK");
    }

    {
        let (tx_data, mut rx_data) = channel().unwrap();
        let (tx_signal, mut rx_signal) = channel().unwrap();
        let child = with_async_write
            .spawn_tokio(tx_data, tx_signal)
            .await
            .unwrap();
        rx_signal.recv().await.unwrap();
        for i in 0..1000 {
            assert_eq!(rx_data.recv().await.unwrap().unwrap(), i);
        }
        child.join().await.unwrap();
        println!("with_async_write OK");
    }
}
