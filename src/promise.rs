//! A background task whose result a synchronous loop (the egui frame, the CLI) can poll.
//! Replaces the `poll-promise` crate, which has had no release since 0.3.0 (RPSN-0010).

use std::cell::OnceCell;
use std::future::Future;
use std::sync::mpsc;

pub struct Promise<T> {
    rx: mpsc::Receiver<T>,
    value: OnceCell<T>,
}

impl<T: Send + 'static> Promise<T> {
    /// Runs the future on the current tokio runtime. Must be called inside one.
    pub fn spawn_async(future: impl Future<Output = T> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        tokio::spawn(async move {
            // The receiver is gone only if the caller dropped the promise; nobody wants the result then.
            let _ = tx.send(future.await);
        });

        Self {
            rx,
            value: OnceCell::new(),
        }
    }
}

impl<T> Promise<T> {
    /// The result, once the task has finished. Never blocks.
    pub fn ready(&self) -> Option<&T> {
        if self.value.get().is_none() {
            if let Ok(value) = self.rx.try_recv() {
                let _ = self.value.set(value);
            }
        }
        self.value.get()
    }

    /// Waits for the task to finish and returns its result.
    pub fn block_and_take(self) -> T {
        match self.value.into_inner() {
            Some(value) => value,
            // The sender is dropped without sending only if the task panicked, and then there is no result to give.
            None => self.rx.recv().expect("the background task panicked"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[test]
    fn ready_is_none_until_the_task_finishes() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let (go, wait) = oneshot::channel::<()>();
        let promise = Promise::spawn_async(async move {
            wait.await.unwrap();
            7
        });
        assert!(promise.ready().is_none());

        go.send(()).unwrap();
        while promise.ready().is_none() {
            std::thread::yield_now();
        }
        assert_eq!(promise.ready(), Some(&7));
        assert_eq!(promise.block_and_take(), 7);
    }

    #[test]
    fn block_and_take_waits_for_the_result() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let (go, wait) = oneshot::channel::<()>();
        let promise = Promise::spawn_async(async move {
            wait.await.unwrap();
            "done"
        });
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            go.send(()).unwrap();
        });
        assert_eq!(promise.block_and_take(), "done");
    }
}
