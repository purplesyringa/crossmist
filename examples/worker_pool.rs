#![feature(trait_upcasting)] // needed for trait intersection to work as expected

use anyhow::{anyhow, bail, Result};
use crossmist::{
    func, lambda,
    tokio::{duplex, Child, Duplex},
    BindValue, FnOnceObject, Object,
};
use std::any::Any;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

// Simulate trait intersection
trait AnyObject: Any + Object {}
impl<T: Any + Object> AnyObject for T {}

type TypeErased = Box<dyn AnyObject>;

struct Worker {
    // The worker process will receive tasks as functions returning Any output and return their
    // output via the channel.
    channel:
        Duplex<Box<dyn FnOnceObject<(), Output = TypeErased> + Send + Sync + 'static>, TypeErased>,

    child: Child<()>,
}

struct WorkerPool {
    workers_sender: Option<mpsc::UnboundedSender<Worker>>,
    workers_receiver: Mutex<mpsc::UnboundedReceiver<Worker>>,
}

impl WorkerPool {
    pub async fn new(concurrency: usize) -> Result<Self> {
        let (workers_sender, workers_receiver) = mpsc::unbounded_channel();
        for _ in 0..concurrency {
            let (ours, theirs) = duplex()?;
            let child = worker.spawn_tokio(theirs).await?;
            if workers_sender
                .send(Worker {
                    channel: ours,
                    child,
                })
                .is_err()
            {
                bail!("Queue initialization failed");
            }
        }
        Ok(Self {
            workers_sender: Some(workers_sender),
            workers_receiver: Mutex::new(workers_receiver),
        })
    }

    pub async fn run<Output: Object + 'static>(
        &self,
        func: impl FnOnceObject<(), Output = Output> + Send + Sync + 'static,
    ) -> Result<Output> {
        let workers_sender = self
            .workers_sender
            .as_ref()
            .ok_or_else(|| anyhow!("Pool is closed"))?;
        let wrapped_function: Box<
            dyn FnOnceObject<(), Output = TypeErased> + Send + Sync + 'static,
        > = Box::new(_wrapped_function.bind_value(func));
        let mut workers_receiver = self.workers_receiver.lock().await;
        let mut worker_obj = workers_receiver
            .recv()
            .await
            .ok_or_else(|| anyhow!("All workers are dead"))?;
        drop(workers_receiver);
        let output = worker_obj.channel.request(&wrapped_function).await?;
        let output = *(output as Box<dyn Any>).downcast().unwrap();
        if workers_sender.send(worker_obj).is_err() {
            bail!("Failed to put worker back to queue");
        }
        Ok(output)
    }

    async fn close(self) -> Result<()> {
        drop(self.workers_sender);
        let mut workers_receiver = self.workers_receiver.lock().await;
        while let Some(mut worker_obj) = workers_receiver.recv().await {
            drop(worker_obj.channel);
            worker_obj.child.join().await?;
        }
        Ok(())
    }
}

#[func]
fn _wrapped_function<
    Output: Object + 'static,
    Func: FnOnceObject<(), Output = Output> + Send + Sync + 'static,
>(
    func: Func,
) -> TypeErased {
    Box::new(func())
}

#[func]
#[tokio::main(flavor = "current_thread")]
async fn worker(
    mut channel: Duplex<
        TypeErased,
        Box<dyn FnOnceObject<(), Output = TypeErased> + Send + Sync + 'static>,
    >,
) {
    while let Some(func) = channel.recv().await.unwrap() {
        channel.send(&func()).await.unwrap();
    }
}

#[crossmist::main]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let pool = Arc::new(WorkerPool::new(4).await?);
    let mut tasks = Vec::new();
    for x in 1..=5 {
        for y in 1..=5 {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move {
                let func = lambda! { move(x: i32, y: i32) || -> i32 { x + y } };
                println!("{x} + {y} = {}", pool.run(func).await?);
                Result::<()>::Ok(())
            }));
        }
    }
    for task in tasks {
        task.await??;
    }
    Arc::try_unwrap(pool)
        .or_else(|_| bail!("Pool is still in use"))?
        .close()
        .await?;
    Ok(())
}
