use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::net::UnixStream;

// https://docs.rs/tokio/latest/tokio/net/index.html




struct WriteReqmap {
    ws_write: String,
    reqmap: HashMap<u64, oneshot::Sender<String>>,
}

pub struct WsRouter {
    write_reqmap: Arc<Mutex<WriteReqmap>>,
}