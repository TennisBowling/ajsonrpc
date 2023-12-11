use std::{sync::{Arc, Mutex}, error::Error, str::FromStr};
use futures_util::stream::SplitSink;
use http::{Request, Uri};
use tokio::sync::oneshot;
use websocket_lite::{Message, Opcode};
use websocket_lite::AsyncNetworkStream;
use websocket_lite::MessageCodec;
use std::collections::HashMap;
use futures::StreamExt;
use tokio_util;

struct WriteReqmap<T> {
    ws_write: SplitSink<T, MessageCodec>, _>,
    reqmap: HashMap<u64, oneshot::Sender<String>>,
}

pub struct WsRouter {
    write_reqmap: Arc<Mutex<WriteReqmap>>,
}

impl WsRouter {
    pub async fn new(node: &str, jwt: Option<String>) -> Result<WsRouter, Box<dyn Error>> {
        let builder = websocket_lite::ClientBuilder::new(&node)?;
        if let Some(jwt) = jwt {
            builder.add_header("Authorization".to_string(), format!("Bearer {}", jwt));
        }

        let mut ws_stream = builder.async_connect().await.unwrap();

        let (mut ws_write, mut ws_read) = ws_stream.split();
        tracing::info!("Websocket connection to {} established.", node);

        let write_reqmap = Arc::new(Mutex::new(WriteReqmap{
            ws_write,
            reqmap: HashMap::new(),
        }));

        

        Ok(router)
    }


}
