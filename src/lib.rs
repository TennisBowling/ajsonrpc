use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};
use futures::{stream::SplitSink, StreamExt, SinkExt};
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::Message};
use tracing;
use tokio::{sync::oneshot, net::TcpStream, sync::Mutex};
use url::Url;


fn drop<T>(_: T) {}

pub enum WsError {
    Timeout,
    Other(Box<dyn Error>),
}

struct WriteReqmap {
    ws_write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    reqmap: HashMap<u64, oneshot::Sender<String>>,
}

pub struct WsRouter {
    write_reqmap: Arc<Mutex<WriteReqmap>>,
}

impl WsRouter {
    pub async fn new(node: String) -> Result<WsRouter, Box<dyn Error>> {
        let url = Url::parse(&node).unwrap();

        let (ws, _) = tokio_tungstenite::connect_async(url).await?;
        let (ws_write, ws_read) = ws.split();
        tracing::info!("Websocket connection to {} established.", node);

        let write_reqmap = Arc::new(Mutex::new(WriteReqmap{
            ws_write,
            reqmap: HashMap::new(),
        }));
        let write_reqmap_clone = write_reqmap.clone(); // Clone for closure

        let router = WsRouter{
            write_reqmap: write_reqmap,
        };
        
        let read_loop = ws_read.for_each_concurrent(None, move |msg| {
            let write_reqmap = write_reqmap_clone.clone(); // Clone for closure
            
            async move {
                match msg {
                    Ok(msg) => {
                        let msg = msg.into_text().unwrap();
                        if msg.is_empty() {
                            return;
                        }

                        let resp: serde_json::Value  = serde_json::from_str(&msg).unwrap();

                        let payload_id = resp["id"].as_str().unwrap().parse::<u64>().unwrap();
                        
                        if let Some(tx) = write_reqmap.lock().await.reqmap.remove(&payload_id) {
                            tx.send(msg).unwrap();
                        } else {
                            tracing::warn!("No corresponding sender found for payload_id: {}", payload_id);
                        }
                    },
                    Err(e) => {
                        tracing::error!("Error reading message: {}", e);
                    }
                }
            }
        });

        tokio::spawn(read_loop);

        Ok(router)
    }

    // make sure that the payload_id is the same id your using in your json rpc request
    pub async fn send(&self, req: String, payload_id: u64) -> Result<oneshot::Receiver<String>, WsError> {

        let (tx, rx) = oneshot::channel();
        let req = Message::Text(req);
        let mut write_reqmap = self.write_reqmap.lock().await;
        write_reqmap.reqmap.insert(payload_id, tx);
        // send the req to the node
        write_reqmap.ws_write.send(req).await.map_err(|e| WsError::Other(Box::new(e)))?;
        drop(write_reqmap); // drop here to yield the lock as soon as possible
        Ok(rx)
    }

    // makes + waits for response
    pub async fn make_request(&self, req: String, payload_id: u64) -> Result<String, WsError> {
        Ok(self.send(req, payload_id).await?.await.map_err(|e| WsError::Other(Box::new(e))))?
    }

    pub async fn make_request_timeout(&self, req: String, payload_id: u64, timeout: Duration) -> Result<String, WsError> {
        let rx = self.send(req, payload_id).await?;
        tokio::time::timeout(timeout, rx).await.map_err(|_| WsError::Timeout)?.map_err(|e| WsError::Other(Box::new(e)))
    }
}