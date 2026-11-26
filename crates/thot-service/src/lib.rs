//! Local API services can inherit a systemd listening socket and exit between activities.
use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
};
use std::{
    future::Future,
    io,
    net::SocketAddr,
    os::fd::FromRawFd,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;

pub async fn serve<F, Fut>(address: SocketAddr, app: Router, busy: F) -> io::Result<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = bool> + Send,
{
    let activated = std::env::var("LISTEN_PID").ok().as_deref()
        == Some(&std::process::id().to_string())
        && std::env::var("LISTEN_FDS").as_deref() == Ok("1");
    if !activated {
        return axum::serve(TcpListener::bind(address).await?, app).await;
    }
    // systemd passes the single configured listener as fd 3.
    let socket = unsafe { std::net::TcpListener::from_raw_fd(3) };
    let inherited = socket;
    let socket = inherited.try_clone()?; // CLOEXEC on the retained descriptor.
    drop(inherited);
    if socket.local_addr()? != address {
        return Err(io::Error::other(
            "socket ListenStream must match the configured bind address",
        ));
    }
    socket.set_nonblocking(true)?;
    let last = Arc::new(Mutex::new(Instant::now()));
    let activity = last.clone();
    let app = app.layer(middleware::from_fn(move |request: Request, next: Next| {
        let activity = activity.clone();
        async move {
            *activity.lock().unwrap() = Instant::now();
            let response = next.run(request).await;
            *activity.lock().unwrap() = Instant::now();
            response
        }
    }));
    let busy = Arc::new(busy);
    loop {
        let last_poll = last.clone();
        let active = busy.clone();
        axum::serve(TcpListener::from_std(socket.try_clone()?)?, app.clone())
            .with_graceful_shutdown(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    let idle = last_poll.lock().unwrap().elapsed();
                    if idle >= Duration::from_secs(60) && !active().await {
                        break;
                    }
                }
            })
            .await?;
        // A request already being accepted can enqueue work while the listener drains.
        if !busy().await && last.lock().unwrap().elapsed() >= Duration::from_secs(60) {
            return Ok(());
        }
    }
}
